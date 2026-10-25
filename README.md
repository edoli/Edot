# Edot

가벼운 텍스트 노트 앱입니다. Rust와 GPUI로 구현하며 폴더·노트 트리, 여러 탭, 자동 저장, 현재·전체 노트 검색과 바꾸기를 제공합니다.

## 빌드와 실행

### Windows

Windows에서 rustup의 stable Rust, C++ 빌드 도구·Windows SDK, Python 3.11 이상이 필요합니다. 

```powershell
python tools/configure_path_remap.py
cargo build --release --target-dir target
.\target\release\edot.exe
```

### Linux

rustup의 stable Rust와 Python 3.11 이상이 필요합니다. CI와 같은 Ubuntu 24.04 기준으로 빌드·실행 의존성을 설치한 뒤 빌드합니다.

```bash
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential clang cmake pkg-config python3 \
  libasound2-dev libfontconfig1-dev libfreetype6-dev libwayland-dev \
  libx11-xcb-dev libxkbcommon-x11-dev libvulkan1 libssl-dev libcurl4

python3 tools/configure_path_remap.py
cargo build --release --target-dir target
./target/release/edot
```

### macOS

rustup의 stable Rust, Python 3.11 이상, Xcode와 Metal 컴파일러가 필요합니다. CI는 Apple Silicon의 macOS 15에서 빌드합니다. Xcode를 설치하고 초기 설정을 마친 뒤 Metal 컴파일러를 확인하고 빌드합니다.

```bash
if ! xcrun -sdk macosx metal --version; then
  xcodebuild -downloadComponent MetalToolchain
fi
xcrun -sdk macosx metal --version

python3 tools/configure_path_remap.py
cargo build --release --target-dir target
./target/release/edot
```

## Space와 저장 위치

Space는 노트와 관련 파일을 담는 폴더입니다. `--space <폴더>` → `EDOT_SPACE_DIR` → 마지막 Space → 기본 위치 순서로 열 폴더를 결정합니다. 사이드바 하단의 Space 목록에서 생성·등록·전환할 수 있으며, 목록의 삭제는 등록만 제거하고 폴더와 노트를 보존합니다.

| OS | 기본 Space | 전역 설정 폴더 |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\Edot\spaces\Default` | `%APPDATA%\Edot` |
| macOS | `~/Library/Application Support/Edot/spaces/Default` | `~/Library/Application Support/Edot` |
| Linux | `${XDG_DATA_HOME:-~/.local/share}/edot/spaces/Default` | `${XDG_CONFIG_HOME:-~/.config}/edot` |

Space 내부 파일은 다음과 같습니다.

| 경로 | 내용 |
| --- | --- |
| `notes/{id}.txt` | UTF-8 본문. 파일명의 ID는 10진수입니다. |
| `.edot/tree.tsv` | `EDOT5` 형식의 트리. 각 행에 ID·부모 ID·순서·종류(`note`/`folder`)·일반 텍스트 제목을 저장합니다. ID와 부모 ID는 패딩 없는 11자 base64url입니다. |
| `.edot/session.bin` | 탭 순서·활성 탭, 사이드바 표시·폭, 트리 펼침 상태. 24바이트 헤더와 8바이트 little-endian ID 배열을 사용합니다. |
| `.edot/space.ini` | Space 이름. |
| `images/` | 본문에 삽입한 이미지 파일. |
| `trash/` | 삭제한 항목의 메타데이터(`.meta`)와 노트 본문(`.txt`). 앱의 휴지통에서 복원·영구 삭제·비우기를 할 수 있습니다. |

## 배포

GitHub Actions의 `.github/workflows/ci.yml`은 main push, main 대상 PR, 수동 실행과 `v*` 태그 push에서 동작합니다. CI가 Cargo 버전의 숫자 세 부분(각 0~65535, 선행 0 없음)을 검사하고 태그가 있으면 정확히 `v<Cargo 버전>`인지 확인합니다. 버전 검사 → stable Rust의 `release-dist` 빌드 → OS별 패키징을 수행하며 태그 실행만 GitHub Release를 공개합니다.

Windows x64, macOS Apple Silicon, Linux x64를 배포합니다. Windows CRT는 정적으로 링크하고 Linux 앱 라이브러리는 정적으로 묶되 GUI·GPU·libc 등 시스템 라이브러리는 사용합니다. macOS 앱은 인증서 없이 ad-hoc 서명하며 Apple 공증은 수행하지 않습니다.

파일명은 `edot-windows.zip`, `edot-mac.zip`, `edot-linux.zip`, `edot-installer-exe.exe`, `edot-installer-dmg.dmg`, `edot-installer-deb.deb`, `edot-installer-appimage.AppImage`으로 고정합니다. Windows/Linux ZIP은 실행 파일만, macOS ZIP은 `.app` 번들을 포함합니다. 패키징 단계에서 OS별 `package-checksums-*.json`을 만들고 Release 작업에서 모든 필수 파일과 SHA-256을 검증한 뒤 위 7개 파일만 공개합니다. 이전 버전을 재배포해도 더 높은 버전의 latest를 대체하지 않습니다.

앱 업데이트는 설정창을 열 때 확인하며, 결과를 5분간 재사용합니다. GitHub Releases API에서 대상 파일과 SHA-256·크기를 읽어 검증합니다. 설치 형태에 맞춰 Windows 설치 파일·macOS DMG·Linux DEB 또는 AppImage를 적용하고, 그 외 포터블은 OS별 ZIP의 실행 파일을 교체한 뒤 다시 실행합니다. 검증 정보가 없거나 다운로드 검증에 실패하면 업데이트하지 않습니다.

릴리스 준비는 Cargo.toml 버전 변경 → Cargo.lock 갱신 → 검증 → 두 파일을 포함한 커밋 → 같은 버전의 태그 생성 → main과 태그 push 순서로 수행합니다. 별도 버전 변경·커밋·태그 자동화는 없습니다.
