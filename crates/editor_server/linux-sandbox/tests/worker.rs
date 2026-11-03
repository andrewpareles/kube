#![cfg(target_os = "linux")]
use base64::{Engine, engine::general_purpose::STANDARD};
use linux_sandbox::validate_glb;
use serde_json::{Value, json};
use std::{collections::HashMap, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdout, Command},
};

// These tests run the actual worker, shell, Python, and Blender, without an editor or network.
struct Session {
    child: Child,
    output: BufReader<ChildStdout>,
    next_request_id: u64,
}
impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_linux_sandbox_worker"))
            .env_clear()
            .env("EDITOR_TEST_SECRET", "not a real credential")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            output,
            next_request_id: 0,
        }
    }
    // Replies repeat their request's id, since requests run at the same time and can finish in any order.
    async fn send(&mut self, mut request: Value) -> u64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        request["id"] = json!(id);
        self.child.stdin.as_mut().unwrap().write_all(format!("{request}\n").as_bytes()).await.unwrap();
        id
    }
    async fn receive(&mut self) -> Value {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(125), self.output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("{error}: {line}"))
    }
    async fn request(&mut self, request: Value) -> Value {
        let id = self.send(request).await;
        let response = self.receive().await;
        assert_eq!(response["id"], id, "{response}");
        response
    }
    async fn command(&mut self, command: &str) -> String {
        let response = self.request(json!({"operation":"run", "command":command, "timeout_seconds":120})).await;
        assert_eq!(response["result"]["exit_code"], 0, "{response}");
        response["result"]["output"].as_str().unwrap().to_string()
    }
    async fn read_file(&mut self, path: &str) -> Vec<u8> {
        let response = self.request(json!({"operation":"read_file", "path":path})).await;
        STANDARD
            .decode(response["result"]["base64"].as_str().unwrap_or_else(|| panic!("{response}")))
            .unwrap()
    }
}

// A zombie has exited and is only waiting to be reaped, so it doesn't count as running.
fn is_running(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|status| !status.lines().any(|line| line.starts_with("State:") && line.contains('Z')))
}
async fn wait_until_stopped(pid: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while is_running(pid) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "Requires Linux root, Blender, and a 64 MiB /dev/shm."]
async fn shell_subprocesses_and_blender_share_a_workspace_and_keep_restrictions() {
    std::fs::write("/tmp/editor-sandbox-test-secret", "not a real credential").unwrap();
    let mut session = Session::start();
    session
        .command(&format!(
            "python3 - <<'PY'\nsupervisor_pid={}\nsecret_path='/tmp/editor-sandbox-test-secret'\n{}\nPY",
            session.child.id().unwrap(),
            include_str!("sandbox.py")
        ))
        .await;
    session.command("printf 'persistent' > state.txt").await;
    assert_eq!(session.command("cat state.txt").await, "persistent");
    // Save an animated rig, reopen it in another process, change a bone pose, and render two frames.
    session
        .command(&format!(
            "cat > build.py <<'PY'\n{}\nPY\nblender --background --factory-startup --threads 2 --python-exit-code 1 --python build.py",
            include_str!("scene.py")
        ))
        .await;
    session.command("blender --background --disable-autoexec model.blend --threads 2 --python-exit-code 1 --python-expr \"import bpy; bpy.context.scene.frame_set(8); assert bpy.data.objects['Rig'].pose.bones['Bone'].rotation_euler.z > 0.5; bpy.context.scene.render.filepath='frame8.png'; bpy.ops.render.render(write_still=True)\"").await;
    for path in ["frame1.png", "frame8.png"] {
        let response = session.request(json!({"operation":"read_image", "path":path})).await;
        let image = STANDARD
            .decode(response["result"]["base64"].as_str().unwrap_or_else(|| panic!("{response}")))
            .unwrap();
        assert!(image.starts_with(b"\x89PNG"), "{path}");
    }
    let glb = session.read_file("model.glb").await;
    validate_glb(&glb).unwrap();
    let model = gltf::Gltf::from_slice(&glb).unwrap();
    assert_eq!(model.skins().count(), 1);
    assert!(model.animations().count() > 0);
    assert!(session.read_file("model.blend").await.starts_with(b"BLENDER"));
    // A later command reopens the saved source and preserves its original animation.
    session
        .command(&format!(
            "cat > edit.py <<'PY'\n{}\nPY\nblender --background --disable-autoexec model.blend --threads 2 --python-exit-code 1 --python edit.py",
            include_str!("edit.py")
        ))
        .await;
    let edited_glb = session.read_file("edited.glb").await;
    validate_glb(&edited_glb).unwrap();
    let model = gltf::Gltf::from_slice(&edited_glb).unwrap();
    assert_eq!(model.skins().count(), 1);
    assert!(model.animations().count() >= 2);
    assert_ne!(session.read_file("model.blend").await, session.read_file("edited.blend").await);
    // A model's .glb written into the workspace, the way a downloaded source arrives, is imported into a new scene.
    let response = session
        .request(json!({"operation":"write_file", "path":"source.glb", "base64":STANDARD.encode(&glb)}))
        .await;
    assert!(response["result"].is_object(), "{response}");
    session
        .command("blender --background --factory-startup --threads 2 --python-exit-code 1 --python-expr \"import bpy; bpy.ops.wm.read_factory_settings(use_empty=True); bpy.ops.import_scene.gltf(filepath='source.glb'); assert any(o.type == 'ARMATURE' for o in bpy.data.objects)\"")
        .await;
    std::fs::remove_file("/tmp/editor-sandbox-test-secret").unwrap();
}

#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn image_reads_reject_links_traversal_fifos_and_jpegs() {
    let mut session = Session::start();
    session
        .command("ln -s /etc secret; ln -s /etc/hostname link; mkfifo pipe; python3 -c \"from PIL import Image; Image.new('RGB', (2, 2)).save('preview.jpg')\"")
        .await;
    for path in ["/etc/hostname", "../editor-secret", "secret/hostname", "link", "pipe", "preview.jpg"] {
        let response = session.request(json!({"operation":"read_image", "path":path})).await;
        assert!(response["error"].is_string(), "{path}: {response}");
    }
    // Failed commands return useful output and don't discard the workspace.
    let response = session
        .request(json!({"operation":"run", "command":"echo intentional-error; exit 7", "timeout_seconds":120}))
        .await;
    assert_eq!(response["result"]["exit_code"], 7);
    assert!(response["result"]["output"].as_str().unwrap().contains("intentional-error"));
    session.command("test -f preview.jpg").await;
}

#[tokio::test]
#[ignore = "Requires Linux root; checks descendants and immediate workspace cleanup after SIGKILL."]
async fn killing_the_worker_cleans_background_grandchildren_and_files() {
    let mut session = Session::start();
    let workspace = session.command("pwd").await.trim().to_string();
    // This command is still running, with a child and a grandchild, when its supervisor is killed.
    session
        .send(json!({"operation":"run", "command":"sh -c 'sleep 300 & wait' > background.log 2>&1 & echo $! > child.pid; wait", "timeout_seconds":120}))
        .await;
    let pid: i32 = session
        .command("while [ ! -s child.pid ]; do sleep 0.05; done; cat child.pid")
        .await
        .trim()
        .parse()
        .unwrap();
    // The nested shell has started a grandchild before we kill its supervisor.
    let grandchild = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let children = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")).unwrap();
            if let Some(child) = children.split_whitespace().next() {
                break child.to_string();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    session.child.kill().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let is_any_running = [pid.to_string(), grandchild.clone()].iter().any(|pid| is_running(pid));
            if !is_any_running && !std::path::Path::new(&workspace).exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn commands_run_at_the_same_time() {
    let mut session = Session::start();
    // The first command waits for a file that only the second creates, so it finishes only if both run at once.
    let waiting = session
        .send(json!({"operation":"run", "command":"while [ ! -f go ]; do sleep 0.05; done; echo done", "timeout_seconds":60}))
        .await;
    let creating = session.send(json!({"operation":"run", "command":"touch go", "timeout_seconds":60})).await;
    let mut response_of_id = HashMap::new();
    for _ in 0..2 {
        let response = session.receive().await;
        response_of_id.insert(response["id"].as_u64().unwrap(), response);
    }
    assert_eq!(response_of_id[&creating]["result"]["exit_code"], 0, "{}", response_of_id[&creating]);
    assert_eq!(response_of_id[&waiting]["result"]["output"], "done\n", "{}", response_of_id[&waiting]);
}

#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn background_processes_end_with_their_command() {
    let mut session = Session::start();
    let pid = session.command("sleep 300 > /dev/null 2>&1 & echo $!").await.trim().to_string();
    wait_until_stopped(&pid).await;
    // The workspace and its files outlive the processes.
    session.command("printf 'kept' > state.txt").await;
    assert_eq!(session.command("cat state.txt").await, "kept");
}

#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn a_command_past_its_timeout_is_stopped_and_the_workspace_is_kept() {
    let mut session = Session::start();
    session.command("printf 'kept' > state.txt").await;
    let response = session
        .request(json!({"operation":"run", "command":"echo started; sleep 300 & echo $! > sleep.pid; wait", "timeout_seconds":2}))
        .await;
    assert_eq!(response["result"]["is_timed_out"], true, "{response}");
    assert_eq!(response["result"]["exit_code"], Value::Null, "{response}");
    assert_eq!(response["result"]["output"], "started\n", "{response}");
    wait_until_stopped(session.command("cat sleep.pid").await.trim()).await;
    assert_eq!(session.command("cat state.txt").await, "kept");
    for timeout_seconds in [0, 601] {
        let response = session
            .request(json!({"operation":"run", "command":"true", "timeout_seconds":timeout_seconds}))
            .await;
        assert!(response["error"].as_str().unwrap().contains("timeout"), "{response}");
    }
}

// Node stops a run when its tool call is stopped, and the run ends the way one past its timeout does.
#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn a_stopped_run_ends_with_its_background_processes_and_the_workspace_is_kept() {
    let mut session = Session::start();
    session.command("printf 'kept' > state.txt").await;
    let run = session
        .send(json!({"operation":"run", "command":"sleep 300 & echo $! > sleep.pid; wait", "timeout_seconds":600}))
        .await;
    let pid = session
        .command("while [ ! -s sleep.pid ]; do sleep 0.05; done; cat sleep.pid")
        .await
        .trim()
        .to_string();
    let stop = session.send(json!({"operation":"stop_run", "run_id":run})).await;
    let mut response_of_id = HashMap::new();
    for _ in 0..2 {
        let response = session.receive().await;
        response_of_id.insert(response["id"].as_u64().unwrap(), response);
    }
    assert!(response_of_id[&stop]["result"].is_object(), "{}", response_of_id[&stop]);
    assert_eq!(response_of_id[&run]["result"]["exit_code"], Value::Null, "{}", response_of_id[&run]);
    assert_eq!(response_of_id[&run]["result"]["is_timed_out"], false, "{}", response_of_id[&run]);
    wait_until_stopped(&pid).await;
    assert_eq!(session.command("cat state.txt").await, "kept");
}

#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn written_and_read_files_stay_inside_the_workspace() {
    let mut session = Session::start();
    let response = session
        .request(json!({"operation":"write_file", "path":"model.glb", "base64":STANDARD.encode(b"first")}))
        .await;
    assert!(response["result"].is_object(), "{response}");
    // A second write replaces the file, and commands can still change it.
    session
        .request(json!({"operation":"write_file", "path":"model.glb", "base64":STANDARD.encode(b"second")}))
        .await;
    session.command("printf ' changed' >> model.glb").await;
    let response = session.request(json!({"operation":"read_file", "path":"model.glb"})).await;
    assert_eq!(response["result"]["base64"], STANDARD.encode(b"second changed"), "{response}");
    session.command("ln -s /etc/hostname link; mkdir folder").await;
    for path in ["/tmp/escaped", "../escaped", "link", "folder"] {
        let response = session
            .request(json!({"operation":"write_file", "path":path, "base64":STANDARD.encode(b"x")}))
            .await;
        assert!(response["error"].is_string(), "{path}: {response}");
    }
    assert!(!std::path::Path::new("/tmp/escaped").exists());
    for path in ["/etc/hostname", "link", "missing"] {
        let response = session.request(json!({"operation":"read_file", "path":path})).await;
        assert!(response["error"].is_string(), "{path}: {response}");
    }
    let response = session
        .request(json!({"operation":"write_file", "path":"big", "base64":STANDARD.encode(vec![0; 16 * 1024 * 1024 + 1])}))
        .await;
    assert!(response["error"].is_string(), "{response}");
}

#[tokio::test]
#[ignore = "Requires Linux root and about 2.5 GiB of free memory."]
async fn running_out_of_memory_stops_only_the_biggest_command() {
    let mut session = Session::start();
    session.command("printf 'kept' > state.txt").await;
    // Together the two commands hold more than the sandbox's 2 GiB, so the watchdog stops the one holding the most.
    let biggest = session
        .send(json!({"operation":"run", "command":"python3 -c \"import time; held = b'x' * (1200 * 1024 * 1024); open('big-ready', 'w').close(); time.sleep(60)\"", "timeout_seconds":120}))
        .await;
    let smaller = session
        .send(json!({"operation":"run", "command":"while [ ! -f big-ready ]; do sleep 0.05; done; python3 -c \"import time; held = b'x' * (1000 * 1024 * 1024); time.sleep(3)\"; echo finished", "timeout_seconds":120}))
        .await;
    let mut response_of_id = HashMap::new();
    for _ in 0..2 {
        let response = session.receive().await;
        response_of_id.insert(response["id"].as_u64().unwrap(), response);
    }
    assert_eq!(response_of_id[&biggest]["result"]["exit_code"], Value::Null, "{}", response_of_id[&biggest]);
    assert_eq!(response_of_id[&smaller]["result"]["output"], "finished\n", "{}", response_of_id[&smaller]);
    assert_eq!(session.command("cat state.txt").await, "kept");
}

// When the whole container runs out of memory, Linux stops a command before the editor.
#[tokio::test]
#[ignore = "Requires Linux root and the sandbox kernel capabilities."]
async fn linux_stops_commands_first_when_the_container_runs_out_of_memory() {
    let mut session = Session::start();
    session
        .send(json!({"operation":"run", "command":"echo $$ > sleep.pid; exec sleep 300", "timeout_seconds":600}))
        .await;
    let pid = session
        .command("while [ ! -s sleep.pid ]; do sleep 0.05; done; cat sleep.pid")
        .await
        .trim()
        .to_string();
    assert_eq!(std::fs::read_to_string(format!("/proc/{pid}/oom_score_adj")).unwrap().trim(), "1000");
}

#[tokio::test]
#[ignore = "Requires Linux root; tests normal stdin closure without waiting for another session."]
async fn closing_stdin_removes_the_workspace() {
    let mut session = Session::start();
    let workspace = session.command("pwd").await.trim().to_string();
    drop(session.child.stdin.take());
    let _ = session.child.wait().await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while std::path::Path::new(&workspace).exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
