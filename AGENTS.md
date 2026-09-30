# AGENTS.md — инструкция для ИИ-агентов

Ты работаешь с **Open Antigravity** — модифицированной портативной сборкой
для Antigravity 2.0 (Google IDE) v2.17.0.3+: без лицензионного ключа,
с прокси, вшитым в бинарник, с портативным режимом. Владелец: TG `@bismark_dev`,
GitHub `https://github.com/keelbismark/OpenAntigravity`. Rust, ~32 000 строк, один
бинарник: GUI (eframe/egui 0.36, glow+wgpu) + терминальный TUI (ratatui) в нём же.

Перед изменениями прочти этот файл целиком. Правила ниже — не рекомендации,
а инварианты: их нарушение ломает сборки, лицензии или дистрибуцию.

---

## 1. Критические инварианты (НЕ ЛОМАТЬ)

1. **Версия Cargo = 2.17.0, не меняй её.** Изменение версии в `Cargo.toml`
   **пересаливает все лицензионные ключи** (правило I2 проекта-оригинала).
   Версия релиза приложения хранится в едином файле-источнике **`VERSION`**
   (например, `1.1.0`) в корне репозитория и передаётся в компилятор через
   переменную `AG_FULL_VERSION` (автоматически считывается в `build.rs`
   с директивой `cargo:rerun-if-changed=VERSION`).
2. **`AG_BUILTIN_PROXY` вшивается build.rs с XOR-обфускацией.** Пароль от
   прокси никогда не попадает в лог сборки (только `host:port`). Не «упрости»
   обфускацию, не выводи секрет, не убери проверку формата
   (`логин:пароль@хост:порт`).
3. **`panic = "abort"`** в `[profile.release]`. Код не может полагаться на
   unwind: арифметику с внешними данными делай `checked_*` (см. пример
   `decode_chunked` в `update.rs`).
4. **MSRV >= 1.95** (eframe 0.36.1 заперт в Cargo.lock). Не предлагай
   `cargo update --precise` для даунгрейда egui — исходники писаны под API 0.36.
5. **`cfg(relay)` / `cfg(exits)`** — приватные модули оригинала (`src/relay.rs`,
   `src/exits.rs`, ключи `.relay_key`/`.exits`). Их нет в репо, включать их
   нельзя, код-хосты в `proxy.rs`/`upstream.rs` не трогать: локальный прокси
   без них просто форвардит во вшитый апстрим напрямую.
6. **`canary.rs` — маркировка происхождения** (`NOTICE`, `STATIC_CANARY`,
   `RELEASE_TOKEN`). Не удаляй и не «оптимизируй»: это осознанная часть
   лицензионной чистоты (см. док-комментарий файла).
7. **Windows-only механики — только под `cfg`.** DNS-слой (NRPT, релей :53),
   `taskkill` в `portcheck.rs`, повышение прав — Windows-специфика. Linux-порт
   это осознанно не умеет (см. «Известные ограничения» ниже).
8. **Юзер-фacing тексты — на русском**, комментарии в коде — на английском и
   объясняют «почему», а не «что». Сообщения об ошибках — для человека,
   без `.rs:line` (см. `without_source_location` в `gui/mod.rs`).

## 2. Карта модулей

```
VERSION            — единый источник правды версии релиза (SSOT: 1.1.0)
main.rs            — вход: флаги CLI, выбор GUI/TUI, фолбэки (терминал -> TUI)
core/              — ядро приложения:
                     ops.rs (воркер: Cmd/Event, тяжёлая логика, профили прокси),
                     settings.rs (настройки и пресеты прокси),
                     portable.rs (портативный режим data/), update.rs (обновления),
                     utils.rs (утилиты), ls_log.rs (парсинг логов)
patcher/           — модификация IDE:
                     binary.rs (побайтовый патч Language Server / CLI),
                     ide.rs (патч компонентов IDE / Electron),
                     asar.rs (работа с asar-архивами), watchdog.rs (наблюдение за откатами)
network/           — сеть, DNS и прокси:
                     proxy.rs (локальный HTTP/CONNECT сервер), upstream.rs (апстрим-прокси),
                     routes.rs (маршрутизация и бенчмарки), doh.rs (DNS-over-HTTPS),
                     resolvers.rs (резолверы), dns*.rs (forwarder/client/NRPT),
                     hosts.rs (пиннинг /etc/hosts на Linux), net.rs, loopback.rs,
                     egress.rs, endpoint.rs, gate.rs
platform/          — системный слой и OS-специфика:
                     autostart.rs (автозапуск Linux/Windows),
                     service.rs (systemd юнит на Linux / задачи Windows),
                     portcheck.rs (проверка и освобождение портов),
                     elevate.rs (повышение прав pkexec/UAC),
                     diag.rs (диагностика --check), shortcut.rs (ярлыки на Рабочий стол и меню),
                     health.rs (мониторинг здоровья процессов), notify.rs (уведомления)
gui/               — eframe: mod.rs (App, каналы, фоновый пинг), main_view.rs (Doctor, пресеты),
                     status.rs (Facts->Action), renderer.rs (glow/wgpu выбор),
                     theme.rs, widgets.rs (интерактивный metric_chip), tray.rs (системный трей)
tui/               — ratatui/crossterm терминальный интерфейс
canary.rs          — маркировка происхождения и токен релиза
build.rs           — чтение VERSION, вшивание AG_BUILTIN_PROXY (XOR), AG_FULL_VERSION
build_portable.sh  — сборщик Linux-дистрибутива (glibc/musl)
build_appimage.sh  — упаковщик Linux AppImage
package_release.sh — сборщик релизных архивов
```

## 3. Архитектурные правила

- **Всё синхронное.** Никакого async-рантайма: потоки + `std::sync::mpsc`.
  HTTP — руками поверх `rustls` (см. `update.rs`/`doh.rs`), не тяни reqwest.
- **UI не блокируется никогда.** Долгие операции — в потоки воркера;
  фоновой поток, положивший сообщение в канал, обязан разбудить egui
  (`ctx.request_repaint()`), иначе баннер/статус никто не увидит
  (egui спит до repaint).
- **Один владелец записи настроек** — воркер. UI читает снапшот и не пишет
  файл параллельно (иначе гонка «кто последний сохранил»).
- **Facts -> Action** (`gui/status.rs`): UI не решает сам, что можно нажать;
  статус приносит готовое действие (`Action::EnableAll/Repair/KillHolder/...`).
  TUI и GUI рисуют одно и то же решение.
- **Единый источник версии (`VERSION`)**: версия релиза меняется только в
  файле `VERSION` в корне репо. `build.rs` подтягивает её в бинарник, скрипты
  упаковки используют её для имён архивов, а CI/CD обновляет `version.json`.
- **Каждая нетривиальная зависимость** должна иметь комментарий-обоснование в
  Cargo.toml (почему нельзя стандартной библиотекой). Смотри примеры там же.
- Тесты не ходят в сеть: живые проверки помечай `#[ignore]` с объяснением.
- Имена тестов — поведенческие (`a_stale_copy_of_ours_gets_the_kill_button...`).

## 4. Сборка / тест / дистрибуция

```bash
# Быстрая проверка (на хосте или в Podman/Docker, если нет хостового тулчейна C/C++):
podman run --rm -v "$PWD":/src -v cargo-cache:/usr/local/cargo/registry -w /src rust:1-bookworm cargo check
podman run --rm -v "$PWD":/src -v cargo-cache:/usr/local/cargo/registry -w /src rust:1-bookworm cargo test
# ~315 тестов; на Linux-песочнице 2 известных фейла (гонки портов,
# воспроизводятся на чистом оригинале — не «чинить» вслепую)
```

Целевые сборки:

| Что | Как |
|---|---|
| Windows exe | `cargo build --release` или `build_portable.cmd` |
| Linux статика (TUI-only, любые glibc) | `bash build_portable.sh` (musl по умолчанию) |
| **Linux GUI & AppImage (Steam Deck и др.)** | `bash build_portable.sh` (glibc) + `bash build_appimage.sh` |
| **Автосборка релизов (все платформы)** | `git tag vX.Y.Z && git push origin vX.Y.Z` (GitHub Actions) |

Полные сценарии — в `BUILD_PORTABLE.md` (§8: WSL2, Docker на VPS, SteamOS,
траблшутинг). Держи его в синхроне при изменении сборки.

**Мусль-ловушка (объясни пользователю, если он её увидит):** статический musl
не может dlopen системные Wayland/X11-библиотеки (они glibc) — GUI-окно у
musl-сборки не откроется нигде, только TUI. Это свойство статики, не баг.
GUI на Deck = glibc-сборка со старым дистрибутивом (bookworm, glibc 2.36
<= 2.41 SteamOS).

Дистрибутив (`dist_portable_linux/`): `open_antigravity`, `launch.sh` (sudo-гвард,
noexec-фолбэк в `~/.local/share/openantigravity`, авто-TUI без графики),
`icon.png`, `README.txt`.

## 5. Механизм обновлений и релизы

- **Единый источник версии**: корень репозитория, файл **`VERSION`**.
- **Выпуск релиза**:
  1. Изменить версию в файле `VERSION` (например `1.2.0`).
  2. Создать и запушить тег: `git tag v1.2.0 && git push origin v1.2.0`.
  3. GitHub Actions (`.github/workflows/release.yml`) автоматически:
     - Соберёт Linux AppImage и переносимый `.tar.gz`.
     - Соберёт Windows `.zip` со всеми ярлыками и bat-файлами запуска.
     - Создаст GitHub Release, сгенерирует `SHA256SUMS.txt` и `version.json`.
     - Обновит `docs/version.json` и сайт GitHub Pages.
- **Сайт (`docs/index.html`)**: динамически запрашивает GitHub Releases API
  (`api.github.com/repos/.../releases/latest`) и сразу обновляет бейдж релиза
  и ссылки на загрузку всех файлов.
- Адрес фида автообновлений в приложении: `AG_UPDATE_URL` (runtime-override >
  вшитый через `option_env!` > выкл).

## 6. Портативность и данные

- `AG_PORTABLE=1` вшит сборщиком; маркер `portable.flag` рядом с exe.
- Все данные — в `data/` рядом с бинарником (`settings.json`, пресеты прокси, отчёты, кэш).
  Удаление `data/` = полный сброс. Права root не нужны нигде.
- Отчёт (`save_report`, `report_dir()`): портативно -> `data/`, иначе Desktop.
- Смена прокси пользователем: блок «Прокси-сервер» и менеджер профилей.

## 7. Известные ограничения Linux-порта

- DNS-слой на Linux реализован через избирательный пиннинг в `/etc/hosts`
  с запросом привилегий через Polkit (`pkexec`) при необходимости и автоматическим
  сбросом кэша `systemd-resolved` (на Windows используется NRPT).
- В линуксовой песочнице два теста падают гонкой портов — они падают и на
  чистом оригинале; не «чинить» вслепую.

## 8. Стиль работы

- Сначала читай код вокруг места правки; этот код густо покрыт комментариями
  «почему» — они часть контракта.
- Правки минимальные и точечные; не переформатируй чужой код.
- При редактировании файлов никогда не читай один и тот же файл повторно по кругу.
- Пользователь строит на Windows, Linux собирает в WSL2/Docker/Podman — ошибки
  окружения объясняй простыми словами, пути давай копипаст-готовые.
- После правок: `cargo check` + `cargo test`; GUI-тексты не ломай по ширине,
  перенос строки в строковых литералах делай `\`-продолжением как в коде.
