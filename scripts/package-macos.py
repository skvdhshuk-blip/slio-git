#!/usr/bin/env python3
"""Build the desktop or MAS package from exactly one Cargo artifact.

The shell entry points select a channel. Distribution signing is validated before
Cargo runs; sandbox-test is an explicit, separately named local test artifact.
"""
import argparse
import datetime
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
APP_NAME = "slio-git"
BUNDLE_ID = "com.slio.git"
PRIVATE_SYMBOL = b"CGSSetWindowBackgroundBlurRadius"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def output(*args):
    return run(*args, stdout=subprocess.PIPE).stdout


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def identities(codesigning=False):
    args = ["security", "find-identity", "-v"]
    if codesigning:
        args.extend(["-p", "codesigning"])
    return re.findall(r'\) ([0-9A-F]{40}) "([^"]+)"', output(*args).decode())


def choose_identity(candidates, requested, label, prefixes):
    candidates = [(key, name) for key, name in candidates if name.startswith(prefixes)]
    if requested:
        candidates = [(key, name) for key, name in candidates if requested in (key, name)]
    if len(candidates) != 1:
        raise ValueError(f"Set {label} to one valid signing identity; found {len(candidates)} matches")
    return candidates[0]


def validate_profile(profile, entitlements, certificate_sha1, team, now):
    """Validate data independently of Keychain / build execution (also unit tested)."""
    if profile.get("ExpirationDate", datetime.datetime.min) <= now:
        raise ValueError("Provisioning profile is expired")
    if profile.get("ProvisionedDevices") or profile.get("ProvisionsAllDevices"):
        raise ValueError("Distribution needs a Mac App Store profile")
    if team not in profile.get("TeamIdentifier", []):
        raise ValueError("Provisioning profile team does not match DEVELOPMENT_TEAM")
    certificates = {hashlib.sha1(cert).hexdigest().upper() for cert in profile.get("DeveloperCertificates", [])}
    if certificate_sha1.upper() not in certificates:
        raise ValueError("Signing certificate is not included in provisioning profile")
    allowed = profile.get("Entitlements", {})
    expected = f"{team}.{BUNDLE_ID}"
    if entitlements.get("com.apple.application-identifier") != expected:
        raise ValueError("Entitlement application identifier does not match bundle and team")
    if entitlements.get("com.apple.developer.team-identifier") != team:
        raise ValueError("Entitlement team does not match signing team")
    if allowed.get("com.apple.application-identifier") != expected:
        raise ValueError("Provisioning profile is not for this application")
    if allowed.get("get-task-allow") or allowed.get("com.apple.security.get-task-allow"):
        raise ValueError("Distribution profile allows debugging")
    if entitlements.get("com.apple.security.app-sandbox") is not True:
        raise ValueError("MAS requires app-sandbox entitlement")
    # Sandbox capabilities are code-signing permissions, not all are repeated in
    # profiles. Restricted application/keychain/team entitlements must match.
    for key, value in entitlements.items():
        if key.startswith("com.apple.security.") and key not in allowed:
            continue
        permitted = allowed.get(key)
        if isinstance(value, list):
            if not isinstance(permitted, list) or any(not any(fnmatch.fnmatchcase(item, pattern) for pattern in permitted) for item in value):
                raise ValueError(f"Provisioning profile does not allow {key}")
        elif permitted != value:
            raise ValueError(f"Provisioning profile does not allow {key}")


def preflight(mode):
    entitlements = plistlib.loads((ROOT / "packaging/macos/slio-git.entitlements").read_bytes())
    if mode == "sandbox-test":
        # An ad-hoc sandbox test cannot claim an App Store application/team or
        # keychain group; it keeps the same bundle ID and sandbox file grants.
        for key in ("com.apple.application-identifier", "com.apple.developer.team-identifier", "keychain-access-groups"):
            entitlements.pop(key, None)
        return {"entitlements": entitlements, "app_identity": "-", "installer_identity": None, "profile": None, "profile_uuid": None}
    team = os.environ.get("DEVELOPMENT_TEAM", "M2WM2NJP68")
    profile_path = Path(os.environ.get("MAS_PROVISIONING_PROFILE", ROOT / "packaging/macos/slio-git-mas.provisionprofile")).resolve()
    if not profile_path.is_file():
        raise ValueError(f"Missing provisioning profile: {profile_path}")
    app_hash, app_name = choose_identity(identities(True), os.environ.get("CODESIGN_IDENTITY"), "CODESIGN_IDENTITY", ("Apple Distribution:", "3rd Party Mac Developer Application:"))
    installer_hash, installer_name = choose_identity(identities(), os.environ.get("INSTALLER_IDENTITY"), "INSTALLER_IDENTITY", ("3rd Party Mac Developer Installer:",))
    if f"({team})" not in app_name or f"({team})" not in installer_name:
        raise ValueError("App and installer signing identities must match DEVELOPMENT_TEAM")
    profile = plistlib.loads(output("security", "cms", "-D", "-i", profile_path))
    validate_profile(profile, entitlements, app_hash, team, datetime.datetime.utcnow())
    return {"entitlements": entitlements, "app_identity": app_hash, "installer_identity": installer_hash, "profile": profile_path, "profile_uuid": profile.get("UUID")}


def source_identity():
    commit = output("git", "-C", ROOT, "rev-parse", "HEAD").decode().strip()
    changes = output("git", "-C", ROOT, "status", "--porcelain=v1", "--untracked-files=all")
    # A dirty build remains identifiable, including newly added source files.
    hasher = hashlib.sha256(output("git", "-C", ROOT, "diff", "HEAD", "--binary"))
    for path in output("git", "-C", ROOT, "ls-files", "--others", "--exclude-standard", "-z").split(b"\0"):
        if path:
            hasher.update(path)
            file = ROOT / os.fsdecode(path)
            if file.is_file():
                hasher.update(file.read_bytes())
    return {"source_commit": commit, "source_dirty": bool(changes), "source_changes_sha256": hasher.hexdigest() if changes else None}


def cargo_binary(target, cache, features):
    command = ["cargo", "build", "--locked", "--no-default-features", "--release", "-p", "src-ui", "--target", target, "--message-format=json-render-diagnostics"]
    if features:
        command.extend(["--features", ",".join(features)])
    env = dict(os.environ, CARGO_TARGET_DIR=str(cache))
    process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True)
    binaries = []
    for line in process.stdout:
        event = json.loads(line)
        if event.get("reason") == "compiler-message":
            print(event["message"].get("rendered", ""), file=sys.stderr, end="")
        if event.get("reason") == "compiler-artifact" and event["target"]["name"] == "src-ui" and event.get("executable"):
            if sorted(event.get("features", [])) != sorted(features):
                raise ValueError("Cargo reported unexpected channel features")
            binaries.append(Path(event["executable"]).resolve())
    if process.wait() != 0:
        raise ValueError("Cargo build failed; no package was assembled")
    if len(binaries) != 1 or not binaries[0].is_relative_to(cache.resolve()):
        raise ValueError("Cargo did not return exactly one executable in this channel's cache")
    return binaries[0]


def bundle(binary, destination, version, build_number, channel, identity):
    contents = destination / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    (contents / "Resources").mkdir()
    shutil.copy2(binary, contents / "MacOS" / APP_NAME)
    icon = ROOT / "src-ui/assets/AppIcon.icns"
    shutil.copy2(icon, contents / "Resources/AppIcon.icns")
    info = dict(CFBundleDevelopmentRegion="en" if channel == "mas" else "zh_CN", CFBundleDisplayName=APP_NAME, CFBundleExecutable=APP_NAME, CFBundleIdentifier=BUNDLE_ID, CFBundleInfoDictionaryVersion="6.0", CFBundleName=APP_NAME, CFBundlePackageType="APPL", CFBundleShortVersionString=version, CFBundleVersion=build_number, CFBundleIconFile="AppIcon", LSMinimumSystemVersion="12.0", NSHighResolutionCapable=True)
    if channel == "mas":
        info.update(LSApplicationCategoryType="public.app-category.developer-tools", ITSAppUsesNonExemptEncryption=False)
        for folder in ("DocumentsFolder", "DesktopFolder", "DownloadsFolder", "RemovableVolumes", "NetworkVolumes"):
            info[f"NS{folder}UsageDescription"] = "slio-git opens Git repositories you select."
    (contents / "Info.plist").write_bytes(plistlib.dumps(info))
    (contents / "PkgInfo").write_bytes(b"APPL????")
    (contents / "Resources/build-info.json").write_text(json.dumps(identity, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("channel", choices=["desktop", "mas"])
    parser.add_argument("--mode", choices=["distribution", "sandbox-test"], default="distribution")
    parser.add_argument("--preflight-only", action="store_true")
    args = parser.parse_args()
    target = os.environ.get("MACOS_TARGET") or re.search(r"^host: (.+)$", output("rustc", "-vV").decode(), re.M)[1]
    if target not in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
        raise ValueError(f"Unsupported macOS target: {target}")
    arch = target.split("-")[0]
    if os.environ.get("MACOS_ARCH", arch) != arch:
        raise ValueError("MACOS_ARCH disagrees with MACOS_TARGET")
    signing = preflight(args.mode) if args.channel == "mas" else None
    if args.preflight_only:
        print(f"Preflight passed: {args.channel}, {args.mode}, {target}")
        return
    package = output("cargo", "metadata", "--no-deps", "--format-version=1", "--manifest-path", ROOT / "Cargo.toml")
    version = next(item["version"] for item in json.loads(package)["packages"] if item["name"] == "src-ui")
    build_number = (ROOT / "packaging/macos/BUILD_NUMBER").read_text().strip() if args.channel == "mas" else version
    if not re.fullmatch(r"[0-9]+(?:\.[0-9]+){0,2}", build_number):
        raise ValueError("Invalid build number")
    features = ["app-store"] if args.channel == "mas" else []
    identity = dict(source_identity(), channel=args.channel, mode=args.mode if signing else "desktop", features=features, version=version, build_number=build_number, architecture=arch, target=target)
    if signing:
        identity.update(signing_identity=signing["app_identity"], profile_uuid=signing["profile_uuid"], entitlements=signing["entitlements"])
    cache_root = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()
    cache = cache_root / "package" / args.channel / arch
    binary = cargo_binary(target, cache, features)
    identity["cargo_binary_sha256"] = digest(binary)
    if source_identity() != {key: identity[key] for key in ("source_commit", "source_dirty", "source_changes_sha256")}:
        raise ValueError("Source changed during build; rerun before packaging")
    if signing and PRIVATE_SYMBOL in binary.read_bytes():
        raise ValueError("Private CGS blur symbol remains in the final binary")
    actual_arches = output("lipo", "-archs", binary).decode().split()
    if actual_arches != [{"aarch64": "arm64", "x86_64": "x86_64"}[arch]]:
        raise ValueError("Binary architecture disagrees with build target")
    out = ROOT / "dist" / args.channel / arch
    if signing:
        out /= args.mode
    out.mkdir(parents=True, exist_ok=True)
    # Every invocation assembles in its own directory. An earlier successful
    # artifact remains intact if building, signing or packaging fails.
    with tempfile.TemporaryDirectory(prefix=".assemble-", dir=out) as temporary:
        staging = Path(temporary)
        app = staging / f"{APP_NAME}.app"
        bundle(binary, app, version, build_number, args.channel, identity)
        artifacts = []
        if signing:
            entitlement_path = staging / "signing.entitlements"
            entitlement_path.write_bytes(plistlib.dumps(signing["entitlements"]))
            if signing["profile"]:
                shutil.copy2(signing["profile"], app / "Contents/embedded.provisionprofile")
            run("codesign", "--force", "--options", "runtime", "--entitlements", entitlement_path, "--sign", signing["app_identity"], app)
            run("codesign", "--verify", "--strict", "--verbose=2", app)
            actual = plistlib.loads(output("codesign", "-d", "--entitlements", ":-", app))
            if actual != signing["entitlements"]:
                raise ValueError("Signed entitlements do not match preflight")
            if args.mode == "distribution":
                pkg = staging / f"{APP_NAME}-appstore-{arch}.pkg"
                run("productbuild", "--component", app, "/Applications", "--sign", signing["installer_identity"], pkg)
                run("pkgutil", "--check-signature", pkg)
                artifacts.append(pkg)
        else:
            # A valid ad-hoc signature is necessary for local Apple Silicon use.
            run("codesign", "--force", "--sign", "-", app)
            dmg_root = staging / "dmg-root"
            dmg_root.mkdir()
            shutil.copytree(app, dmg_root / app.name, symlinks=True)
            (dmg_root / "Applications").symlink_to("/Applications")
            dmg = staging / f"{APP_NAME}-macos-{arch}.dmg"
            run("hdiutil", "create", "-volname", APP_NAME, "-srcfolder", dmg_root, "-ov", "-format", "UDZO", dmg)
            artifacts.append(dmg)
        identity["binary_sha256"] = digest(app / "Contents/MacOS" / APP_NAME)
        identity["artifacts"] = {artifact.name: digest(artifact) for artifact in artifacts}
        manifest = staging / f"{APP_NAME}-{args.channel}-{arch}.manifest.json"
        manifest.write_text(json.dumps(identity, indent=2) + "\n")
        # Only our exact per-channel destinations are replaced.
        for artifact in [app, *artifacts, manifest]:
            destination = out / artifact.name
            if destination.is_dir():
                shutil.rmtree(destination)
            os.replace(artifact, destination)
        print(f"App: {out / app.name}\nManifest: {out / manifest.name}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Packaging failed: {error}")
