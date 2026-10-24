# Что нового / What's new

Ropeway показывает раздел своей версии после обновления. В заметках к
выпуску на GitHub — тот же текст.

## 0.7.0

### Русский

- Новая главная страница «Мои устройства»: избранные устройства сверху, с быстрыми кнопками — файлы, экран, управление, чат.
- У каждого устройства своя страница: отправка файлов, трансляция экрана, управление компьютером, чат и все передачи файлов.
- Чаты переехали в отдельную вкладку.
- Добавить устройство можно по QR-коду (на телефоне — «Сканировать QR»), по временному коду из шести цифр или по IP.
- Настройка «Автоматически отображать моё устройство»: если её выключить, устройство скрыто, и подключиться к нему можно только по IP, QR или коду.
- Сообщения можно удалять — у себя или у всех, а чат — очищать.
- Показ экрана работает на Windows 10, в том числе LTSC, и даёт понятную подсказку, если macOS не разрешает запись экрана.
- Тема Fluent на Windows 10 больше не прозрачная, а переключение тем не двоится и не повторяется при повторном нажатии.
- Ссылка на исходный код на GitHub в Настройках, обновлённый установщик для Windows.
- В терминале: `ropeway code`, `ropeway visible on|off`, `ropeway connect <код>`.

### English

- A new start page, My devices: favourite devices at the top with quick buttons for files, screen, control and chat.
- Every device has its own page: send files, share the screen, control the computer, chat, and all file transfers with it.
- Chats moved to their own tab.
- Add a device by QR code (Scan QR code on a phone), by a six-digit temporary code, or by IP address.
- The "Show my device automatically" setting: turned off, the device is hidden and can only be connected by IP, QR or code.
- Messages can be deleted, for you or for everyone, and chats cleared.
- Screen sharing works on Windows 10, LTSC included, and explains what to do when macOS does not allow screen recording.
- The Fluent theme is no longer see-through on Windows 10, and switching themes no longer doubles the picture or replays when the theme is already on.
- A link to the source code on GitHub in Settings, and a refreshed Windows installer.
- In the terminal: `ropeway code`, `ropeway visible on|off`, `ropeway connect <code>`.

## 0.6.1

### Русский

- Обновления прямо из приложения: Ropeway сам находит новую версию, предлагает её установить и показывает, что изменилось. Проверить вручную можно в Настройках.
- Безопасность: ключ устройства на Linux и macOS теперь доступен только вашему пользователю.
- Если перестать доверять устройству, которое смотрит ваш экран, показ для него сразу прекращается.
- Доверенные устройства больше не смотрят экран без вопроса. Это можно разрешить в Настройках, в разделе «Доверенные устройства».
- Общая папка по умолчанию открыта только доверенным устройствам. Уже настроенные копии не меняются.
- Защита от перегрузки запросами от чужих устройств и от поддельных объявлений в сети.
- Из имён устройств, сообщений и имён файлов убираются невидимые символы, которыми можно подменить отображаемый текст.
- Окно подтверждения доверия предупреждает, если в сети есть устройство с таким же именем.

### English

- Updates from inside the app: Ropeway finds a new version, offers to install it and shows what changed. You can also check by hand in Settings.
- Security: on Linux and macOS the device key is now readable only by your user.
- When you stop trusting a device that is watching your screen, it stops seeing it right away.
- Trusted devices no longer watch your screen without asking. You can allow it in Settings under "Trusted devices".
- The shared folder is open to trusted devices only by default. Copies already set up keep their setting.
- Protection against floods of requests from other devices and against fake announcements on the network.
- Invisible characters that could disguise text are removed from device names, messages and file names.
- The trust dialog warns when another device on the network has the same name.

## 0.6.0

### Русский

- Ropeway для Android, Linux и macOS.
- Управление из терминала: команда `ropeway`.
- Темы Liquid Glass и Retro.

### English

- Ropeway for Android, Linux and macOS.
- Control from the terminal: the `ropeway` command.
- Liquid Glass and Retro themes.
