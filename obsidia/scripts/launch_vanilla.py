#!/usr/bin/env python3
"""Hito 1: lanzar Minecraft Java vanilla en modo offline desde un script.

Uso:
    python scripts/launch_vanilla.py [version] [--username NOMBRE] [--dry-run]

Ejemplos:
    python scripts/launch_vanilla.py 26.3
    python scripts/launch_vanilla.py 1.21.1 --username Steve
"""
import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import urllib.request
import uuid
from pathlib import Path

BASE = Path(os.environ.get("MINECRAFT_DIR", Path.home() / "AppData" / "Roaming" / ".minecraft"))
VERSIONS_DIR = BASE / "versions"
LIBS_DIR = BASE / "libraries"
ASSETS_DIR = BASE / "assets"

RUNTIMES = Path(os.environ.get("MINECRAFT_RUNTIME", Path.home() / "AppData" / "Roaming" / ".minecraft" / "runtime"))
JAVA_CANDIDATES = [
    RUNTIMES / "java-runtime-delta" / "windows-x64" / "java-runtime-delta" / "bin" / "java.exe",
    RUNTIMES / "java-runtime-gamma" / "windows-x64" / "java-runtime-gamma" / "bin" / "java.exe",
    RUNTIMES / "java-runtime-epsilon" / "windows-x64" / "java-runtime-epsilon" / "bin" / "java.exe",
]


def java_for(version: str) -> Path:
    # Prefer a runtime matching the version's needs; fall back to any available.
    if version.startswith("1.20"):
        order = [JAVA_CANDIDATES[1], JAVA_CANDIDATES[0], JAVA_CANDIDATES[2]]
    elif version.startswith("26."):
        order = [JAVA_CANDIDATES[2], JAVA_CANDIDATES[0], JAVA_CANDIDATES[1]]
    else:
        order = [JAVA_CANDIDATES[0], JAVA_CANDIDATES[1], JAVA_CANDIDATES[2]]
    for p in order:
        if p.exists():
            return p
    # Last resort: java on PATH
    return Path("java")


def fetch(url: str) -> bytes:
    print(f"[dl] {url}")
    with urllib.request.urlopen(url, timeout=60) as r:
        return r.read()


def sha1_ok(data: bytes, expected: str | None) -> bool:
    if not expected:
        return True
    return hashlib.sha1(data).hexdigest() == expected


def download(url: str, dest: Path, expected_sha1: str | None = None) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        if expected_sha1:
            try:
                if hashlib.sha1(dest.read_bytes()).hexdigest() == expected_sha1:
                    return
            except OSError:
                pass
        else:
            return
    data = fetch(url)
    if not sha1_ok(data, expected_sha1):
        raise RuntimeError(f"SHA1 mismatch for {url}")
    dest.write_bytes(data)


def get_version_json(version: str) -> dict:
    vdir = VERSIONS_DIR / version
    jpath = vdir / f"{version}.json"
    if jpath.exists():
        return json.loads(jpath.read_text(encoding="utf-8"))
    print(f"[info] version json missing, fetching manifest for {version}")
    manifest = json.loads(fetch("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"))
    entry = next((v for v in manifest["versions"] if v["id"] == version), None)
    if entry is None:
        raise RuntimeError(f"Version {version} not found in manifest")
    vdir.mkdir(parents=True, exist_ok=True)
    data = fetch(entry["url"])
    jpath.write_bytes(data)
    return json.loads(data)


def os_name() -> str:
    s = platform.system().lower()
    if s == "windows":
        return "windows"
    if s == "darwin":
        return "osx"
    return "linux"


def arch_name() -> str:
    m = platform.machine().lower()
    if m in ("amd64", "x86_64"):
        return "x64"
    if m in ("x86", "i386", "i686"):
        return "x86"
    if "arm" in m:
        return "arm64"
    return "x64"


OS = os_name()
ARCH = arch_name()
# Minecraft uses 'x86' for arch field in some rules
MC_ARCH = "x86" if ARCH == "x86" else ("arm64" if ARCH == "arm64" else "x64")


def rules_allow(rules, features) -> bool:
    if not rules:
        return True
    allowed = False
    for r in rules:
        ros = r.get("os", {})
        if ros:
            if "name" in ros and ros["name"] != OS:
                continue
            if "arch" in ros and ros["arch"] != MC_ARCH:
                continue
        rfeat = r.get("features", {})
        if rfeat:
            ok = all(features.get(k, False) == v for k, v in rfeat.items())
            if not ok:
                continue
        action = r.get("action", "allow")
        if action == "allow":
            allowed = True
        else:
            allowed = False
    return allowed


def maven_path(name: str) -> str:
    parts = name.split(":")
    group, artifact, version = parts[0], parts[1], parts[2]
    classifier = parts[3] if len(parts) > 3 else None
    group_path = group.replace(".", "/")
    fname = f"{artifact}-{version}"
    if classifier:
        fname += f"-{classifier}"
    fname += ".jar"
    return f"{group_path}/{artifact}/{version}/{fname}"


def substitute(s: str, variables: dict) -> str:
    out = s
    for k, v in variables.items():
        out = out.replace("${" + k + "}", str(v))
    return out


def process_args(args_list, variables, features) -> list[str]:
    result = []
    for item in args_list:
        if isinstance(item, str):
            result.append(substitute(item, variables))
        elif isinstance(item, dict):
            rules = item.get("rules", [])
            if not rules_allow(rules, features):
                continue
            value = item.get("value")
            if isinstance(value, list):
                for v in value:
                    result.append(substitute(v, variables))
            elif value is not None:
                result.append(substitute(value, variables))
    return result


def offline_uuid(name: str) -> str:
    # Minecraft offline UUID: UUID.nameUUIDFromBytes("OfflinePlayer:"+name)
    import hashlib as _h
    md5 = _h.md5(("OfflinePlayer:" + name).encode("utf-8")).digest()
    # set version 3 bits
    b = bytearray(md5)
    b[6] = (b[6] & 0x0F) | 0x30
    b[8] = (b[8] & 0x3F) | 0x80
    u = uuid.UUID(bytes=bytes(b))
    return str(u)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("version", nargs="?", default="26.3")
    ap.add_argument("--username", default="Player")
    ap.add_argument("--ram", default="2G")
    ap.add_argument("--java", default=None, help="Ruta al ejecutable java a usar")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    version = args.version
    username = args.username

    vjson = get_version_json(version)
    vdir = VERSIONS_DIR / version
    vdir.mkdir(parents=True, exist_ok=True)

    # client jar
    client_info = vjson.get("downloads", {}).get("client", {})
    jar_path = vdir / f"{version}.jar"
    if client_info:
        download(client_info["url"], jar_path, client_info.get("sha1"))
    if not jar_path.exists():
        raise RuntimeError(f"Client jar not found: {jar_path}")

    # libraries
    classpath = [str(jar_path)]
    for lib in vjson.get("libraries", []):
        rules = lib.get("rules", [])
        if not rules_allow(rules, {}):
            continue
        lname = lib["name"].lower()
        if "natives-" in lname:
            if "arm64" in lname and ARCH != "arm64":
                continue
            if "x86" in lname and ARCH != "x86":
                continue
        downloads = lib.get("downloads", {})
        art = downloads.get("artifact")
        if art:
            url = art.get("url")
            sha1 = art.get("sha1")
            rel = art.get("path")
            if not rel:
                rel = maven_path(lib["name"])
            dest = LIBS_DIR / rel
            if url:
                download(url, dest, sha1)
            else:
                # construct default url
                download(f"https://libraries.minecraft.net/{rel}", dest, sha1)
            if dest.exists():
                classpath.append(str(dest))
        else:
            # no downloads block: construct from name
            rel = maven_path(lib["name"])
            dest = LIBS_DIR / rel
            if not dest.exists():
                try:
                    download(f"https://libraries.minecraft.net/{rel}", dest, None)
                except Exception as e:
                    print(f"[warn] could not download {lib['name']}: {e}")
                    continue
            if dest.exists():
                classpath.append(str(dest))

    # natives dir
    natives_dir = vdir / "natives"
    natives_dir.mkdir(parents=True, exist_ok=True)

    asset_index = vjson.get("assetIndex", {})
    asset_index_id = asset_index.get("id", version)

    uuid_val = offline_uuid(username)

    variables = {
        "auth_player_name": username,
        "version_name": version,
        "game_directory": str(BASE),
        "assets_root": str(ASSETS_DIR),
        "assets_index_name": asset_index_id,
        "auth_uuid": uuid_val,
        "auth_access_token": "0",
        "clientid": "",
        "auth_xuid": "",
        "user_type": "legacy",
        "version_type": "release",
        "launcher_name": "Obsidia",
        "launcher_version": "0.1",
        "natives_directory": str(natives_dir),
        "classpath": os.pathsep.join(classpath),
        "resolution_width": "854",
        "resolution_height": "480",
    }

    features = {}

    args_game = process_args(vjson.get("arguments", {}).get("game", []), variables, features)
    args_jvm = process_args(vjson.get("arguments", {}).get("jvm", []), variables, features)

    # Older versions use minecraftArguments string
    if "minecraftArguments" in vjson:
        mc_args = vjson["minecraftArguments"]
        # substitute manually
        mc_args = substitute(mc_args, variables)
        game_args = mc_args.split()
    else:
        game_args = args_game

    main_class = vjson["mainClass"]

    if args.java:
        java = Path(args.java)
    else:
        java = java_for(version)
    cmd = [str(java)]
    # RAM
    cmd.append(f"-Xmx{args.ram}")
    cmd.extend(args_jvm)
    cmd.append(main_class)
    cmd.extend(game_args)

    print("[cmd]")
    print(" ".join(f'"{c}"' if " " in c else c for c in cmd))

    if args.dry_run:
        return

    # Download asset index if missing
    if asset_index.get("url"):
        idx_path = ASSETS_DIR / "indexes" / f"{asset_index_id}.json"
        if not idx_path.exists():
            try:
                download(asset_index["url"], idx_path, asset_index.get("sha1"))
            except Exception as e:
                print(f"[warn] asset index download failed: {e}")

    print("[launch] starting Minecraft...")
    proc = subprocess.Popen(cmd, cwd=str(BASE))
    print(f"[launch] pid={proc.pid}")
    try:
        proc.wait()
    except KeyboardInterrupt:
        proc.terminate()


if __name__ == "__main__":
    main()
