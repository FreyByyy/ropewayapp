# Разработка Ropeway

## Устройство

```
crates/ropeway-core   ядро на Rust без UI и системных API, одно для всех платформ
  identity.rs         постоянный самоподписанный сертификат; id устройства — его отпечаток
  tls.rs              QUIC и взаимный TLS 1.3 с привязкой к отпечатку вместо центра сертификации
  discovery.rs        объявления по UDP broadcast, порт 47800
  protocol.rs         запрос и ответ на отдельном QUIC-потоке, кадры «длина + JSON»
  node.rs             соединения, присутствие, маршрутизация, ограничения для чужих устройств
  chat.rs             история чатов (JSON Lines на диалог)
  transfers.rs        передача файлов и папок: предложение, приём, докачка, прогресс
  shared.rs           общая папка: список, копирование, правки и удаление с одобрением владельца
  fs_util.rs          безопасная работа с путями из сети, закрытые для других файлы
  screen.rs           показ экрана и управление: приглашения, запросы, раздача потока, ввод
  settings.rs         настройки, папки, доверенные устройства
crates/ropeway-media  захват, кодирование и ввод для ПК
  encoder.rs          BGRA → H.264 (OpenH264)
  frames.rs           общий конвейер кадров для всех ОС
  audio.rs            системный звук и микрофон (cpal)
  clipboard.rs        общий буфер обмена (arboard)
  keymap.rs           коды клавиш браузера → коды ОС
  video_windows.rs    Windows Graphics Capture;   input_windows.rs  SendInput
  linux_x11.rs        MIT-SHM и XTest;           linux_wayland.rs  портал и PipeWire
  video_macos.rs      ScreenCaptureKit;           input_macos.rs    CGEvent
src-tauri             оболочка Tauri 2: команды для UI, трей, автозапуск, уведомления,
                      обновления (update.rs), команды в терминале (cli.rs, ipc.rs, daemon.rs)
  android.rs          мост к Kotlin: захват экрана, ввод через Специальные возможности, файлы
src                   интерфейс на React + TypeScript, переводы ru/en, темы
```

Ядро не знает о Tauri. Для новой платформы нужны захват экрана и ввод
в `ropeway-media` (или, как на Android, отдельная реализация `ScreenProvider`).

### Модель безопасности

- Устройство — это ключ: id равен отпечатку сертификата, TLS доказывает
  владение ключом. Решения о доверии принимаются только по этому id, никогда
  по имени или по данным из объявлений.
- При подключении к известному устройству отпечаток закрепляется. Адрес,
  подтверждённый соединением, поддельное объявление не перезапишет.
- От недоверенных устройств ограничены частота запросов, число ожидающих
  предложений и одновременных скачиваний.
- Пути из сети проходят `sanitize_rel`; общая папка не отдаёт ссылки,
  junction’ы и файлы с жёсткими ссылками.
- Ключ устройства и папка данных на Unix закрыты для других (0600 и 0700).
- Обновления подписаны отдельным ключом (minisign через `tauri signer`);
  публичная часть — в `tauri.conf.json`.

## Сборка

Нужны Rust (stable), Node.js 20+ и [NASM](https://www.nasm.us/) в `PATH`
(с ним OpenH264 кодирует кадр 1080p за ~10 мс вместо ~90 мс).

```bash
npm install
```

```bash
npx tauri dev
```

### Windows

Visual Studio Build Tools (MSVC x64) и Windows SDK.

```bash
npx tauri build --bundles nsis
```

Установщик — в `target/release/bundle/nsis/`, программа без установки —
`target/release/ropeway.exe`.

### Linux

Пакеты собираются на Ubuntu 22.04, чтобы работать и на новых системах.
`scripts/linux/provision.sh` готовит машину (подойдёт WSL), затем:

```bash
bash scripts/linux/build.sh
```

AppImage, `.deb` и `.rpm` появятся в `releases/<версия>/`.

### macOS

Собирается на GitHub Actions (`.github/workflows/macos.yml`) при каждом
push в `main`: универсальное приложение для Apple Silicon и Intel. В
артефакте запуска — `.dmg` и пакет обновления `.app.tar.gz`.

### Android

Android SDK и NDK, цели Rust `aarch64-linux-android` и `x86_64-linux-android`.
Ключ подписи APK — `src-tauri/gen/android/ropeway-release.jks` и
`keystore.properties` рядом, в git они не попадают.

```bash
bash scripts/build-android.sh arm64 x86_64
```

## Тесты

Узлы ядра обмениваются сообщениями, файлами и трансляцией через настоящее
зашифрованное соединение: докачка после перезапуска, общая папка, правки
с одобрением, попытки выйти за пределы папки, ссылки, отзыв доверия во время
показа экрана, приём ввода только от разрешённого управляющего.

```bash
cargo test -p ropeway-core -p ropeway-media
```

Скорость кодирования на этом компьютере:

```bash
cargo test --release -p ropeway-media encoding_speed -- --ignored --nocapture
```

### Два экземпляра на одном ПК

Второй экземпляр с отдельной папкой данных не блокируется первым, а удалённые
файлы общей папки кладёт в `removed` своей папки данных, а не в Корзину:

```powershell
$env:ROPEWAY_DATA_DIR = "$env:TEMP\ropeway-second"; .\target\release\ropeway.exe
```

`ROPEWAY_NO_DISCOVERY=1` — экземпляр не объявляет себя в сети; подключайте его
по адресу. Подробный журнал: `RUST_LOG=info,ropeway_core=debug,ropeway_media=debug`.

Данные приложения: `%APPDATA%\app.ropeway.desktop` на Windows,
`~/.local/share/app.ropeway.desktop` на Linux,
`~/Library/Application Support/app.ropeway.desktop` на macOS.

## Выпуск версии

1. Поднять версию в `package.json`, `src-tauri/tauri.conf.json` и `Cargo.toml`,
   добавить раздел в `CHANGELOG.md` (на двух языках: приложение показывает его
   после обновления, а `release.py` берёт его в заметки к релизу).
2. Закоммитить и отправить в `main`: GitHub Actions соберёт macOS.
3. Собрать остальное и разложить в `releases/<версия>/` под такими именами:

   | Файл | Откуда |
   |---|---|
   | `Ropeway-<в>-windows-setup.exe` | `target/release/bundle/nsis/*-setup.exe` |
   | `Ropeway-<в>-windows-portable.exe` | `target/release/ropeway.exe` |
   | `Ropeway-<в>-linux-x86_64.AppImage`, `-linux-amd64.deb`, `-linux-x86_64.rpm` | `scripts/linux/build.sh` |
   | `Ropeway-<в>-android-arm64.apk`, `-android-x86_64.apk` | `scripts/build-android.sh` |
   | `Ropeway-<в>-macos-universal.dmg`, `-macos-universal.app.tar.gz` | артефакт `Ropeway-macos`: `gh run download -n Ropeway-macos -D releases/<в>` |

4. Подписать пакеты обновлений, собрать `latest.json` и выложить релиз:

   ```bash
   python scripts/release.py --publish
   ```

Ключ подписи обновлений — `~/.tauri/ropeway-updater.key` — есть только
на компьютере, с которого выпускаются версии. **Сохраните его копию**:
без него установленные копии Ropeway не примут следующие обновления.
Приложение проверяет `releases/latest/download/latest.json`; если
в `latest.json` нет нужной платформы, обновление для неё не предлагается.
