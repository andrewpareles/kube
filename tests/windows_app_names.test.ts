import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { repoPath } from "./testPaths.ts";

// Windows shows a name in several places that do not come from our own UI: the elevation prompt when
// the installer runs, the Publisher column in Add/Remove Programs, and the executable's Properties
// dialog. Each one reads a different field, and any of them left unset falls back to something that
// does not match the name on the signing certificate, which is what makes an install look untrusted.
const buildScriptSource = fs.readFileSync(repoPath("crates/client/build.rs"), "utf8");
const windowsPackagerSource = fs.readFileSync(repoPath("packaging/client/Packager.windows.toml"), "utf8");
const macosPackagerSource = fs.readFileSync(repoPath("packaging/client/Packager.macos.toml"), "utf8");

const productName = "Kube.new";
const companyName = "Glass Devtools, Inc.";
const copyright = `Copyright (C) ${companyName}`;

/** The value of a top level key in one of the packager configs. */
const readPackagerValue = (packagerSource: string, key: string) => {
    const match = packagerSource.match(new RegExp(`^${key} = "(.*)"$`, "m"));
    return match === null ? undefined : match[1];
};

test("the installer names the same company the signing certificate does", () => {
    // Left unset, cargo-packager takes the middle word of the identifier and the app lists itself as
    // published by "kube", while the certificate it is signed with says Glass Devtools, Inc.
    assert.equal(readPackagerValue(windowsPackagerSource, "publisher"), companyName);

    // The macOS signing identity carries the same company, and is the reason that is the right name.
    const signingIdentity = readPackagerValue(macosPackagerSource, "signing-identity");
    assert.equal(signingIdentity, `Developer ID Application: ${companyName} (ZUZ6WG9BCN)`);
});

test("the installer's elevation prompt names the app rather than its file", () => {
    // cargo-packager writes this into the installer as FileDescription, which is the field Windows
    // shows as the program name. Unset, the prompt reads "Kube-new_x64.exe".
    assert.equal(readPackagerValue(windowsPackagerSource, "description"), productName);
});

test("both platforms carry the same product name, identifier and copyright", () => {
    for (const packagerSource of [windowsPackagerSource, macosPackagerSource]) {
        assert.equal(readPackagerValue(packagerSource, "product-name"), productName);
        assert.equal(readPackagerValue(packagerSource, "copyright"), copyright);
        assert.match(packagerSource, /^identifier = "new\.kube\.client"\s*$/m);
    }
});

test("client.exe carries its own version details", () => {
    // The executable had no version resource at all, so every Windows dialog that named it fell back
    // to the bare filename. cargo-packager sets none of these on the binary, only on the installer.
    assert.match(buildScriptSource, /set\("ProductName", "Kube\.new"\)/);
    assert.match(buildScriptSource, /set\("FileDescription", "Kube\.new"\)/);
    assert.match(buildScriptSource, /set\("CompanyName", "Glass Devtools, Inc\."\)/);
    assert.match(buildScriptSource, /set\("LegalCopyright", "Copyright \(C\) Glass Devtools, Inc\."\)/);
});
