# This script runs through the real sandbox shell and checks inherited restrictions.
import os
import errno
import socket
import subprocess
import ctypes

assert os.getuid() == 65534
assert 'EDITOR_TEST_SECRET' not in os.environ

def denied(operation):
    try:
        operation()
    except OSError as error:
        assert error.errno in (errno.EACCES, errno.EPERM, errno.EROFS)
        return
    raise AssertionError('Sandbox allowed a forbidden operation')

denied(lambda: open(secret_path).read())
os.symlink(secret_path, 'secret-link')
denied(lambda: open('secret-link').read())
denied(lambda: open('/tmp/sandbox-escaped', 'w'))
denied(lambda: open(f'/proc/{supervisor_pid}/environ', 'rb').read())
denied(lambda: socket.socket(socket.AF_INET, socket.SOCK_STREAM))
denied(lambda: socket.socket(socket.AF_INET, socket.SOCK_DGRAM))
denied(lambda: socket.socket(socket.AF_INET6, socket.SOCK_STREAM))
denied(lambda: socket.socket(socket.AF_UNIX, socket.SOCK_STREAM))
denied(lambda: os.kill(supervisor_pid, 0))
denied(lambda: os.memfd_create('outside-workspace'))
libc = ctypes.CDLL(None, use_errno=True)
shm = libc.shmget(0, 4096, 0o1600)
if shm >= 0:
    libc.shmctl(shm, 0, None)
    raise AssertionError('Sandbox allowed persistent kernel shared memory')
assert ctypes.get_errno() == errno.EPERM
denied(os.setsid)
denied(lambda: os.setpgid(0, 0))
# General subprocesses work, and their filesystem/network restrictions are inherited.
child = subprocess.run(['python3', '-c', "import socket; socket.socket()"], capture_output=True)
assert child.returncode != 0 and b'PermissionError' in child.stderr
child = subprocess.run(['python3', '-c', f"open({secret_path!r}).read()"], capture_output=True)
assert child.returncode != 0 and b'PermissionError' in child.stderr
assert subprocess.check_output(['sh', '-c', 'printf child-ok']) == b'child-ok'

try:
    bytearray(4 * 1024 * 1024 * 1024)
except MemoryError:
    pass
else:
    raise AssertionError('Address-space limit was not applied')

try:
    with open('too-large', 'wb') as output:
        output.write(b'x' * (17 * 1024 * 1024))
except OSError as error:
    assert error.errno == errno.EFBIG
else:
    raise AssertionError('File-size limit was not applied')
finally:
    os.unlink('too-large')

