<div align="center">

<img src="icon.png" width="84" height="84" alt="Open Antigravity" />

# Open Antigravity

**Портативная среда для комфортной работы с Google Antigravity в любом регионе**

[![Release](https://img.shields.io/github/v/release/keelbismark/OpenAntigravity?color=ffffff&labelColor=18181b&style=flat-square)](https://github.com/keelbismark/OpenAntigravity/releases)
[![License](https://img.shields.io/badge/license-MIT-ffffff?labelColor=18181b&style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20SteamOS-ffffff?labelColor=18181b&style=flat-square)](#)
[![Website](https://img.shields.io/badge/docs-GitHub%20Pages-ffffff?labelColor=18181b&style=flat-square)](https://keelbismark.github.io/OpenAntigravity/)

[Сайт проекта](https://keelbismark.github.io/OpenAntigravity/) • [Скачать Antigravity](https://antigravity.google/download) • [Быстрый старт](#быстрый-запуск) • [Свой прокси (VPS)](PROXY_SETUP.md)

</div>

> **Open Antigravity** — автономный центр управления для комфортной работы с Google Antigravity (Antigravity 2.0, IDE, CLI). Обеспечивает стабильный доступ к AI-моделям из любого региона под личным Google-аккаунтом без необходимости включать VPN для всей системы.
>
> Редактор загружается отдельно: **[Скачать Google Antigravity с официального сайта](https://antigravity.google/download)**.

---

## Ключевые возможности

| Возможность | Описание |
|---|---|
| 🎯 **Control Center** | Четыре специализированных экрана: **«Обзор»** (статус и пинг), **«Прокси»** (профили и консоль), **«Диагностика»** (Doctor) и **«Журнал»** (события). |
| ⚡ **Live RTT & Инспектор** | Замер задержки отклика до Google AI в реальном времени, график пинга и Live Inspector обращений Language Server к моделям. |
| 🏎️ **Multi-Proxy Racing** | Поддержка цепочки серверов (`ip1:port;ip2:port`). Утилита автоматически соревнует узлы по задержке и выбирает быстрейший. |
| 💻 **CLI и Терминал** | Экспорт переменных `HTTP_PROXY` / `HTTPS_PROXY` в 1 клик для PowerShell, CMD и Bash. Кнопка запуска терминала с преднастроенным шлюзом. |
| 🛡️ **Watchdog & Автопатч** | Фоновый сторож отслеживает обновления Antigravity и автоматически восстанавливает рабочий патч среды. |
| 🔄 **1-Click Обновления** | Проверка и установка новых версий прямо из шапки программы без необходимости переходить в браузер. |
| 🔒 **Изолированный трафик** | Перенаправляются только AI-запросы к Google. Браузер, игры, git, мессенджеры и система работают напрямую на полной скорости сети. |
| 📦 **100% Портативность** | Все настройки, пресеты и логи хранятся в папке `data/` рядом с программой. Удаление папки возвращает систему в исходный вид. |
| 🐧 **Steam Deck & Linux** | Готовый 1-click AppImage для любого Linux и SteamOS (Desktop и Game Mode), а также терминальный TUI-режим по SSH. |

---

## Быстрый запуск

### Графический интерфейс (GUI)

Готовые сборки доступны на странице **[GitHub Releases](https://github.com/keelbismark/OpenAntigravity/releases/latest)**:

| Платформа | Сборка | Запуск |
|---|---|---|
| **Windows 10 / 11** | [OpenAntigravity-windows-portable.zip](https://github.com/keelbismark/OpenAntigravity/releases/latest/download/OpenAntigravity-windows-portable.zip) | Распаковать архив и запустить `OpenAntigravity.exe` |
| **Linux (Steam Deck / Ubuntu / Fedora)** | [OpenAntigravity-x86_64.AppImage](https://github.com/keelbismark/OpenAntigravity/releases/latest/download/OpenAntigravity-x86_64.AppImage) | `chmod +x OpenAntigravity-x86_64.AppImage && ./OpenAntigravity-x86_64.AppImage` |
| **Linux Portable** | [OpenAntigravity-linux-portable.tar.gz](https://github.com/keelbismark/OpenAntigravity/releases/latest/download/OpenAntigravity-linux-portable.tar.gz) | Распаковать архив и запустить `./launch.sh` |

### Терминальный режим (TUI) в одну строку

* **Linux / Steam Deck:**
  ```bash
  curl -fsSL https://raw.githubusercontent.com/keelbismark/OpenAntigravity/main/tui.sh | sh
  ```

* **Windows (PowerShell):**
  ```powershell
  irm https://raw.githubusercontent.com/keelbismark/OpenAntigravity/main/tui.ps1 | iex
  ```

---

## Использование

1. Установите Google Antigravity с официального сайта: [antigravity.google/download](https://antigravity.google/download).
2. Запустите Open Antigravity (на Windows рекомендуется запуск от имени администратора для применения DNS-правил).
3. Нажмите кнопку **«Подключить Antigravity»** на вкладке «Обзор».
4. Откройте Antigravity, авторизуйтесь под личным Google-аккаунтом и начните работу — статус покажет: *«Подключено»*.

### Настройка собственного прокси
* На вкладке **«Прокси»** введите адрес сервера (`логин:пароль@хост:порт`) и нажмите **«Сохранить как профиль»**.
* Для настройки резервирования или автоматического подбора лучшего узла укажите несколько серверов через точку с запятой:
  ```text
  user:pass@ip1:port;user:pass@ip2:port
  ```
  Open Antigravity замерит пинг каждого узла и выберет самый отзывчивый.
* Подробная инструкция по быстрому поднятию своего прокси (Tinyproxy) на VPS за 2 минуты: [**`PROXY_SETUP.md`**](PROXY_SETUP.md).

### Работа с консолью и терминалом (CLI)
* Чтобы утилиты `git`, `curl`, `python`, `npm` работали через шлюз, нажмите кнопку нужной оболочки (**«PowerShell»**, **«CMD»** или **«Bash / Zsh»**) — команда экспорта переменных скопируется в буфер обмена.
* Либо нажмите кнопку **«Запустить терминал»** — откроется окно с уже настроенным окружением.

---

## Консольные команды

Бинарник поддерживает флаги командной строки:

* `--check`, `--diagnose` — выполнение полной сетевой и системной самодиагностики в терминале.
* `--tui`, `--cli` — принудительный запуск в текстовом интерфейсе (TUI).
* `--shortcut` — создание ярлыков на Рабочем столе и в главном меню приложений.
* `--minimized`, `-m`, `--tray` — запуск программы свёрнутой в системный трей.
* `--about`, `--version` — информация о сборке и лицензии.

---

## Сборка из исходников

Для сборки требуется компилятор Rust (**MSRV ≥ 1.95**, в связи с API eframe 0.36.1):

```bash
git clone https://github.com/keelbismark/OpenAntigravity.git
cd OpenAntigravity
cargo build --release
```

Готовый бинарник будет находиться в каталоге `target/release/`.

### Сборочные скрипты

* **Windows:**
  ```bat
  build.cmd            # Сборка портативной папки dist_portable\
  build.cmd --package  # Упаковка в релизный zip-архив
  build.cmd --check    # Проверка исходников и запуск тестов
  ```

* **Linux:**
  ```bash
  bash build.sh            # Сборка glibc-версии
  bash build.sh --musl     # Сборка статической версии (TUI)
  bash build.sh --package  # Упаковка портативного дистрибутива
  bash build_appimage.sh   # Сборка Linux AppImage
  ```

---

## Лицензия

Проект распространяется под свободной лицензией [MIT](LICENSE).  
Open Antigravity не аффилирован с Google LLC. Все товарные знаки принадлежат их законным владельцам.
