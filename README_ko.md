# SmileBASIC 에뮬레이터

[English](README.md) | 한국어 | [日本語](README_ja.md)

Rust+SDL3 조합으로 작성된 SmileBASIC 4 (Nintendo Switch) 호환 에뮬레이터.
아직 개발 중입니다.

비공식 프로젝트이며, SmileBoom 및 Nintendo와는 관계가 없고 승인이나 지원을 받지 않았습니다.
SmileBASIC과 Petit Computer는 SmileBoom Co.Ltd.의 상표이며, Nintendo Switch는 Nintendo의 상표입니다.


## 기능

- [x] SmileBASIC 4 호환 ([differences](docs/using/differences.md))
- [x] 디버깅 기능
- [x] 그래픽 하드웨어 가속
- [ ] SmileBASIC 3 및 Petit Computer(DSi/WiiU) 호환


## 공식 리소스

SmileBASIC의 글꼴, 사운드, 스프라이트는 SmileBoom의 저작물이며, 저작권 보호를 위해 다른 것으로 대체했습니다.

번들 글꼴과 SoundFont로 글자와 음악이 나옵니다. 스프라이트는 빈 그림으로 표시됩니다.

SmileBASIC 4 번들 글꼴, 스프라이트 및 음악/효과음을 사용하려면 SmileBASIC 4 본편 및 업데이트의 암호화되지 않은 덤프가 필요합니다.
SmileBASIC 4를 구매하고 게임을 덤프하셨다면, `smilebasic-import`를 사용하여 공식 리소스를 가져올 수 있습니다.

```sh
smilebasic-import <본편 ROMFS 폴더> <본편 EXEFS 폴더> <업데이트 ROMFS 폴더> <업데이트 EXEFS 폴더>
```

가져온 리소스는 `<데이터 경로>/resources` 아래에 저장됩니다.
자세한 내용은 [docs/using/importing.md](docs/using/importing.md) 를 참조하세요.


## 실행 방법

```sh
# DIRECT 모드로 시작
smilebasic

# 지정된 파일을 PRG0 으로 로드하여 실행
smilebasic PROGRAM

# 전체 화면, 프로젝트 목록, 게임패드 조작 (Simple UI)
smilebasic --simple-ui
```

SmileBASIC 4의 TXT 파일 혹은 UTF-8 텍스트 파일을 지원합니다.
파일이 들어 있는 폴더가 현재 프로젝트가 되므로, 프로그램은 같은 폴더의 파일을 읽고 쓸 수 있습니다 ([자세히](docs/using/differences.md#a-programs-own-folder)).


프로그램의 데이터는 다음 경로에 저장됩니다.
- Windows: `%APPDATA%\smilebasic-emulator`
- Linux: `~/.local/share/smilebasic-emulator`
- macOS: `~/Library/Application Support/smilebasic-emulator`

프로젝트는 데이터 경로의 `projects` 아래에 폴더로 저장됩니다.
프로그램은 UTF-8 텍스트로 저장되며, 프로젝트 관리 화면에서 SmileBASIC 4 포맷으로 가져오거나 내보낼 수 있습니다.


## 단축키

단축키는 기본적으로 SmileBASIC 4의 단축키를 따르며, 에뮬레이터 독자 단축키도 일부 존재합니다.

| 키 | 동작 |
|---|---|
| F1 | 커서 위치의 명령어 도움말 |
| F4 | DIRECT 모드 / 편집 모드 전환 |
| F5 | 프로그램 실행/중단 |
| F8 | 일시정지/메뉴 바 표시 |
| F12 | 프로젝트 목록 열기 |
| Ctrl+Shift+F11 | 전체 화면 |

그 외 단축키는 [Shortcuts](docs/using/shortcuts.md) 도움말을 참조하십시오.


게임패드 조작도 지원하며(컨트롤러 1~4), 게임패드를 연결하지 않아도 키보드로 조작할 수 있습니다.
키보드로 게임패드 조작을 원치 않는 경우, 메뉴 바의 `Run` -> `Use the keyboard as a controller` 를 비활성화하십시오.


## 문서

문서 목록은 [docs/index.md](docs/index.md) 에 있습니다. 소스에서 빌드하는 방법도 여기에서 찾을 수 있습니다.


## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
(`MIT OR Apache-2.0`), at your option. Unless you state otherwise, any
contribution you intentionally submit for inclusion is dual-licensed the same
way, with no additional terms or conditions.

The bundled fonts below are under the SIL Open Font License 1.1:
- `crates/sb-runtime/fonts/fusionpixel`
- `crates/sb-runtime/fonts/galmuri`
- `crates/sb-runtime/fonts/mplus`
- `crates/sb-runtime/fonts/neodgm`
- `crates/sb-runtime/fonts/notosanssc`
- `crates/sb-runtime/fonts/unifont`

Misaki Gothic 2nd in `crates/sb-runtime/fonts/misaki` is under its author's
license, in `LICENSE.txt`.
Izumi Gothic 16 in `crates/sb-runtime/fonts/izumi` is equivalent to public
domain by its author's declaration, in `LICENSE.txt`.
GeneralUser GS in `crates/sb-sdl/sounds` is under its own license, in
`GeneralUser-GS-LICENSE.txt`.

[THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt) holds the license texts of these, and of the Rust
crates and SDL that the binaries link. It goes with every binary that is
given out; `tools/third_party_notices.py` writes it.
