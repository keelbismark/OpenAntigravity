# Настройка собственного прокси (Tinyproxy) за 2 минуты

Для работы Open Antigravity достаточно любого легковесного HTTP/CONNECT прокси-сервера на зарубежном VPS (подойдёт даже самый дешёвый сервер за 1–2$ в месяц с 512 МБ ОЗУ в Европе, США или любой другой стране без ограничений Google).

Ниже описана самая простая и надёжная настройка через **Tinyproxy**.

---

## Вариант 1. Установка на Ubuntu / Debian (рекомендуется)

Подключитесь к вашему VPS по SSH и выполните команды:

### 1. Установка пакета
```bash
sudo apt update && sudo apt install -y tinyproxy
```

### 2. Разрешение внешних подключений
По умолчанию Tinyproxy блокирует все внешние IP-адреса и слушает только `localhost`. Разрешаем доступ из интернета:
```bash
sudo sed -i 's/^Allow 127.0.0.1/#Allow 127.0.0.1/' /etc/tinyproxy/tinyproxy.conf
```

### 3. Установка логина и пароля
Добавьте авторизацию, чтобы вашим прокси не могли воспользоваться посторонние (замените `myuser` и `mypassword` на свои):
```bash
echo "BasicAuth myuser mypassword" | sudo tee -a /etc/tinyproxy/tinyproxy.conf
```

*(Опционально)* Можно изменить стандартный порт `8888` на любой другой:
```bash
sudo sed -i 's/^Port 8888/Port 8080/' /etc/tinyproxy/tinyproxy.conf
```

### 4. Перезапуск службы
```bash
sudo systemctl restart tinyproxy
sudo systemctl enable tinyproxy
```

Готово! Служба активна и готова к работе.

---

## Вариант 2. Запуск через Docker

Если на сервере уже установлен Docker, запустить Tinyproxy с авторизацией можно одной командой:

```bash
docker run -d \
  --name tinyproxy \
  --restart always \
  -p 8888:8888 \
  -e BASIC_AUTH_USER=myuser \
  -e BASIC_AUTH_PASSWORD=mypassword \
  monostream/tinyproxy
```

---

## Проверка работы

С локального компьютера (в терминале или PowerShell) проверьте отклик прокси:

```bash
curl -x http://myuser:mypassword@IP_ВАШЕГО_СЕРВЕРА:8888 https://api.ipify.org
```

Команда должна вернуть IP-адрес вашего зарубежного VPS.

---

## Использование в Open Antigravity

Откройте Open Antigravity и в поле **«Свой прокси»** вставьте строку:

```text
myuser:mypassword@IP_ВАШЕГО_СЕРВЕРА:8888
```

### Настройка резервного прокси (Failover)
Если у вас есть два сервера, укажите их через точку с запятой `;`:

```text
myuser:mypassword@IP_ОСНОВНОГО:8888;myuser:mypassword@IP_РЕЗЕРВНОГО:8888
```

Если основной сервер станет недоступен, Open Antigravity мгновенно переключит трафик на запасной без обрыва диалога в IDE.
