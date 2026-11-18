#!/usr/bin/env bash
set -euo pipefail
output="${1:?output directory required}"
targets="${2:-android/arm64,android/amd64}"
cd "$(dirname "$0")"
mkdir -p "$output/tools" "$output/jni"
go build -o "$output/tools/" golang.org/x/mobile/cmd/gomobile golang.org/x/mobile/cmd/gobind
export PATH="$output/tools:$PATH"
gomobile bind -target="$targets" -androidapi=29 -javapkg=dev.mihomo.vpn -tags=with_gvisor,cmfa -ldflags='-w -s -X=github.com/metacubex/mihomo/constant.Version=v1.19.34' -o "$output/core.aar" ./mobile
unzip -p "$output/core.aar" classes.jar > "$output/classes.jar"
# Replace the directory as a unit so switching ABIs cannot retain stale libraries.
staging="$output/jni-next"
mkdir -p "$staging"
unzip -oq "$output/core.aar" 'jni/*' -d "$staging"
python3 - "$output" <<'PY'
from pathlib import Path
import shutil, sys
root = Path(sys.argv[1])
shutil.rmtree(root / 'jni')
shutil.move(root / 'jni-next/jni', root / 'jni')
shutil.rmtree(root / 'jni-next')
PY
