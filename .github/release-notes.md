## The last release of Veydan Space 4.0

**Veydan Space 5 is released in a new repository:
https://github.com/veydanproject/Veydan-Space/releases/latest**

> **Read every step before you install version 5.** Version 5 does not open
> the local data of 4.0: your data moves only through sync. Once version 5
> is installed, 4.0 can no longer send your data anywhere.

This release only adds the steps of the move to the app (a banner and a card
in Settings). This repository gets no more updates; the downloads of 4.0 stay
here.

How to move:

1. **Turn sync on in 4.0:** Settings → Sync. No sync yet? On a computer no
   cloud is needed: create an empty folder (for example `C:\VeydanSync` or
   `~/VeydanSync`) and choose the type "Local folder" with it. On a phone you
   need WebDAV or S3.
2. **Wait until sync finishes.**
3. **Remember your lock password:** version 5 opens the vault only with it.
   If you use the messenger, save its key: Messenger → Settings → Export
   backup.
4. **Close 4.0 and install version 5 over it** from the link above. Do not
   uninstall 4.0 first: an uninstaller can delete its data, and on a phone it
   always does. On a computer, copy the data folder first as a backup:
   - Windows: `%APPDATA%\net.veydan.space`
   - Linux: `~/.local/share/net.veydan.space`
   - macOS: `~/Library/Application Support/net.veydan.space`
5. **In version 5, first of all connect sync to the same vault** (the same
   folder, WebDAV or S3) with the same password: your data comes from there.
   Then import the messenger key.

Version 5 does not delete the files of 4.0: if something goes wrong,
install 4.0 again and it opens its data as before.

## Последний выпуск Veydan Space 4.0

**Veydan Space 5 выходит в новом репозитории:
https://github.com/veydanproject/Veydan-Space/releases/latest**

> **Прочитайте все шаги до установки версии 5.** Версия 5 не открывает
> локальные данные 4.0: данные переносит только синхронизация. После
> установки версии 5 отправить данные из 4.0 будет уже нечем.

Этот выпуск только добавляет в приложение шаги перехода (баннер и карточку в
настройках). Этот репозиторий больше не обновляется; загрузки 4.0 остаются
здесь.

Как перейти:

1. **Включите синхронизацию в 4.0:** Настройки → Синхронизация. Синхронизации
   ещё нет? На компьютере облако не нужно: создайте пустую папку (например
   `C:\VeydanSync` или `~/VeydanSync`) и выберите тип «Локальная папка» с
   этой папкой. На телефоне нужен WebDAV или S3.
2. **Дождитесь завершения синхронизации.**
3. **Запомните пароль блокировки:** без него версия 5 хранилище не откроет.
   Если пользуетесь мессенджером, сохраните его ключ: Мессенджер → Настройки
   → Экспорт копии.
4. **Закройте 4.0 и установите версию 5 поверх неё** по ссылке выше. Не
   удаляйте 4.0 заранее: деинсталлятор может стереть её данные, а на телефоне
   стирает всегда. На компьютере сначала сделайте копию папки данных:
   - Windows: `%APPDATA%\net.veydan.space`
   - Linux: `~/.local/share/net.veydan.space`
   - macOS: `~/Library/Application Support/net.veydan.space`
5. **В версии 5 первым делом подключите синхронизацию к тому же хранилищу**
   (та же папка, WebDAV или S3) с тем же паролем: данные придут оттуда. Затем
   импортируйте ключ мессенджера.

Файлы 4.0 версия 5 не удаляет: если что-то пойдёт не так, установите 4.0
снова, и она откроет свои данные как раньше.
