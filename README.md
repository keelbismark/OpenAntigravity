<div align="center">

<img src="icon.png" width="96" height="96" alt="Open Antigravity" />

# Open Antigravity

**Портативная среда комфортной работы с Google Antigravity для Windows и Linux**

[![Release](https://img.shields.io/github/v/release/keelbismark/OpenAntigravity?color=24292e&style=flat-square)](https://github.com/keelbismark/OpenAntigravity/releases)
[![Build Status](https://img.shields.io/github/actions/workflow/status/keelbismark/OpenAntigravity/release.yml?style=flat-square)](https://github.com/keelbismark/OpenAntigravity/actions)
[![License](https://img.shields.io/badge/license-MIT-24292e?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20SteamOS-24292e?style=flat-square)](#)
[![Docs](https://img.shields.io/badge/docs-GitHub%20Pages-24292e?style=flat-square)](https://keelbismark.github.io/OpenAntigravity/)

[Сайт проекта](https://keelbismark.github.io/OpenAntigravity/) • [Скачать Antigravity](https://antigravity.google/download) • [Возможности](#ключевые-возможности) • [Быстрый запуск](#быстрый-запуск) • [Инструкция](#использование) • [Свой прокси (Tinyproxy)](PROXY_SETUP.md) • [Сборка](#сборка-из-исходников)

</div>

Open Antigravity — легковесная портативная утилита, обеспечивающая стабильный и свободный доступ к Google Antigravity (Antigravity 2.0, IDE, CLI). Позволяет работать из любого региона с личным аккаунтом Google без необходимости держать включённым сторонний VPN.

Сам редактор загружается отдельно: **[Скачать Google Antigravity (официальный сайт)](https://antigravity.google/download)**.

---

## Ключевые возможности

* **Быстрый старт в один клик** — автоматическая настройка компонентов среды по кнопке «Подключить Antigravity».
* **⚡ Live-пинг задержки в реальном времени** — интерактивный замер задержки (RTT) до API моделей Google с автообновлением в фоне и графиком задержки.
* **🗂️ Менеджер профилей и пресетов прокси** — сохранение неограниченного числа прокси с переключением в 1 клик прямо из интерфейса.
* **🩺 Встроенная диагностика окружения («Doctor»)** — сквозная проверка IDE, статуса патча Language Server, локального шлюза и апстрим-соединения прямо на главном экране.
* **Изолированная маршрутизация** — перенаправляются исключительно обращения к AI-моделям. Браузер, git, консоль и пакетные менеджеры работают напрямую на полной скорости.
* **Резервирование прокси (Failover)** — поддержка цепочки прокси через точку с запятой (`основной;резервный`) с автоматическим переключением при сбоях.
* **Автоконтроль при обновлениях (Watchdog)** — защита от сброса настроек при автоматических обновлениях Antigravity.
* **GUI и TUI в одном бинарнике** — строгий тёмный графический интерфейс и терминальный режим для работы по SSH.
* **Полная портативность** — работа без установки и системных служб. Все настройки и профили изолированы в папке `data/`.
* **Поддержка Steam Deck (SteamOS)** — работа в Game Mode и Desktop Mode, готовый AppImage и интеграция в библиотеку Steam в один клик.

---

## Быстрый запуск

### Графический интерфейс (GUI)

Готовые сборки актуальной версии доступны на странице **[Releases](https://github.com/keelbismark/OpenAntigravity/releases/latest)**:

| Платформа | Формат | Запуск |
|---|---|---|
| **Windows 10 / 11** | [OpenAntigravity_windows_v1.1.0.zip](https://github.com/keelbismark/OpenAntigravity/releases/latest/download/OpenAntigravity_windows_v1.1.0.zip) | Распаковать и запустить `OpenAntigravity.exe` |
| **Linux (Steam Deck / Ubuntu / Fedora)** | [OpenAntigravity-x86_64.AppImage](https://github.com/keelbismark/OpenAntigravity/releases/latest/download/OpenAntigravity-x86_64.AppImage) | `chmod +x OpenAntigravity-x86_64.AppImage && ./OpenAntigravity-x86_64.AppImage` |
| **Linux Portable** | [OpenAntigravity_linux_v1.1.0.tar.gz](https://github.com/keelbismark/OpenAntigravity/releases/latest/download/OpenAntigravity_linux_v1.1.0.tar.gz) | Распаковать и запустить `./launch.sh` |

### Терминальный режим (TUI) в одну строку

**Linux / Steam Deck:**
```bash
curl -fsSL https://raw.githubusercontent.com/keelbismark/OpenAntigravity/main/tui.sh | sh
```

**Windows (PowerShell):**
```powershell
irm https://raw.githubusercontent.com/keelbismark/OpenAntigravity/main/tui.ps1 | iex
```

---

## Использование

1. Установите Google Antigravity (IDE / CLI) с официального сайта: [antigravity.google/download](https://antigravity.google/download).
2. Запустите Open Antigravity (на Windows рекомендуется запуск от имени администратора).
3. Нажмите кнопку **«Подключить Antigravity»** в главном окне.
4. Откройте Antigravity, авторизуйтесь в аккаунте Google и отправьте запрос в чат — индикатор статуса покажет: *«Подключено»*.

### Настройка и профили прокси

В блоке **«Прокси-сервер»** вы можете:
* Ввести собственный прокси-сервер формата: `логин:пароль@хост:порт` или `хост:порт`.
* Настроить цепочку резервирования: `основной_прокси;запасной_прокси`.
* Сохранить текущий прокси как именованный пресет (`+ Сохранить как профиль`).
* Быстро переключаться между серверами кликом по плашкам пресетов («Нидерланды», «Германия», «Свой VPS»).

> 📖 **Инструкция по настройке своего прокси:**  
> Подробное пошаговое руководство по быстрому поднятию личного прокси (Tinyproxy) на VPS за 2 минуты доступно в файле [**`PROXY_SETUP.md`**](PROXY_SETUP.md).

### Диагностика окружения («Doctor»)

Блок **«Диагностика окружения (Doctor)»** отображает статус ключевых узлов:
* Статус обнаружения IDE Google Antigravity.
* Состояние патча Language Server.
* Статус локального прокси-шлюза (`127.0.0.1:45318`).
* Пинг и доступность облачного шлюза Google AI.
* Кнопка **«🩺 Проверить всё»** запускает полную сетевую и системную диагностику с выводом отчёта в журнал.

---

## Консольные команды и диагностика

Бинарник поддерживает флаги командной строки:

* `--check`, `--diagnose` — комплексная сетевая и системная диагностика (self-test).
* `--tui`, `--cli` — принудительный запуск в текстовом интерфейсе (TUI).
* `--shortcut` — создание ярлыков на Рабочем столе и в главном меню приложений.
* `--steam` — добавление Open Antigravity в библиотеку Steam (для Linux/Steam Deck).
* `--about`, `--version` — информация о сборке и лицензии.

---

## Сборка из исходников

Для сборки требуется компилятор Rust (MSRV 1.85+):

```bash
cargo build --release
```

Бинарник будет расположен в `target/release/`.

Для вшивания собственного прокси на этапе сборки:
```bash
AG_BUILTIN_PROXY="логин:пароль@хост:порт" cargo build --release
```

---

## Лицензия

Проект распространяется под лицензией [MIT](LICENSE).
Open Antigravity не аффилирован с Google LLC. Все товарные знаки принадлежат их законным владельцам.

