# GPUI fork 관리

- fork: https://github.com/edoli/zed
- upstream: https://github.com/zed-industries/zed
- 적용 브랜치: `edot-gpui`
- 원본 기준 commit: `279fe070bb389b79652e52065b2f001edcc0b11b`
- 앱 고정 commit: `d910aecc63523831fd947ea0f631d91604e02963`

Zed 원본 Git 이력을 공유하는 fork다. Edot의 일반·테스트 `gpui`와 `gpui_platform`은 같은 Git URL과 전체 40자리 `rev`를 지정한다. 내부 workspace crate도 같은 Git tree에서 가져온다. Cargo.lock을 함께 관리하며 브랜치가 변경되어도 앱은 자동 업그레이드되지 않는다. 앱 빌드에는 별도 fork checkout이 필요하지 않다.

## 유지하는 패치

| 파일 | 변경 |
| --- | --- |
| `crates/gpui/Cargo.toml` | 추가 이미지 코덱, SVG 텍스트·폰트와 내장 래스터를 선택 feature로 분리 |
| `crates/gpui_windows/Cargo.toml` | Windows 백엔드의 추가 이미지 코덱을 선택 feature로 분리 |
| `crates/gpui/src/svg_renderer.rs` | SVG 텍스트·폰트 초기화를 `svg-text`로 조건부 컴파일 |

- `extended-image-formats`: 추가 코덱과 image/rayon. BMP/GIF/JPEG/PNG/WebP는 기본 지원한다.
- `svg-text`: SVG 문자·폰트 로딩·shaping·이모지 폴백.
- `svg-raster-images`: SVG 내부 래스터 이미지.

GPUI 기본 feature에는 위 기능을 모두 포함하고 Edot의 일반·테스트 의존성에서 `default-features=false`로 끈다. `windows-manifest`는 유지한다. SVG 도형과 본문 DirectWrite 렌더링은 유지한다.

workspace의 image/resvg feature를 상속하면 제외 기능이 다시 켜지므로 해당 의존성은 명시적으로 선언한다. upstream 병합 시 그 버전 요구사항을 workspace와 맞춘다. fork의 유지 관리 문서는 `EDOT_GPUI.md`다.

## 원본 업데이트

fork checkout에서 `origin`은 `edoli/zed`, `upstream`은 `zed-industries/zed`를 가리킨다. 현재 로컬 checkout은 upstream main을 추적하고 blob을 필요할 때 다운로드한다. 로컬 변경을 보존한 상태에서 실행한다.

```powershell
git switch edot-gpui
git pull --ff-only origin edot-gpui
git fetch upstream
git merge upstream/main
```

마지막 두 명령은 `git pull --no-rebase upstream main`으로 합칠 수 있다. 일반 `git pull`은 origin의 현재 브랜치를 가져온다.

충돌을 해결할 때 세 선택 feature와 원래 GPUI 기본 동작을 유지하고 API·workspace 의존성 변경을 확인한다. 필요하면 소비 앱의 임시 path 의존성으로 검증하되 최종 manifest에 남기지 않는다. 검증한 브랜치를 push하고 commit을 확인한다.

```powershell
git push origin edot-gpui
git rev-parse HEAD
```

이미 앱에서 사용하는 공개 commit은 force-push로 재작성하지 않는다.

## 앱에 적용하고 검증

1. 앱 `Cargo.toml`의 일반·테스트 `gpui`와 `gpui_platform` 세 `rev`를 같은 새 commit으로 바꾼다.
2. 아래 명령으로 lockfile과 경로 치환 설정을 갱신하고 검사한다. 새 checkout은 rev를 바꾸기 전에 기존 lockfile로 `python tools/configure_path_remap.py`부터 실행한다.
3. 이 문서의 원본 기준·앱 고정 commit을 갱신한다. 앱 동작·구조가 바뀌었다면 IMPLEMENTATION.md의 해당 절도 수정한다.

앱 프로젝트 디렉터리에서:

```powershell
cargo metadata --format-version 1 --filter-platform x86_64-pc-windows-msvc > $null
python tools/configure_path_remap.py
python tools/check_gpui_features.py
cargo fmt --check
cargo clippy --all-targets --target-dir target -- -D warnings
cargo test --target-dir target -- --test-threads=1
cargo check --bin edot --features gpui/default --target-dir target
cargo build --release --target-dir target
python tools/check_binary_paths.py
```

최종 빌드는 path override와 추가 feature 없이 고정 Git 소스를 사용한다. 기능 검사는 manifest·lockfile·Windows 실제 트리의 Git source와 코덱·SVG 제한을 확인한다. 렌더러 변경 시 SVG 아이콘 표시도 확인한다. headless 테스트는 독립 저장소를 사용한다. 위 Clippy 명령은 Edot용이며 fork 자체를 검사할 때는 그 저장소의 `.rules`를 따른다.
