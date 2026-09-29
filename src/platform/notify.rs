//! Кроссплатформенные системные уведомления (Desktop Notifications).
//!
//! Отправка всплывающих сообщений пользователю при работе приложения в фоне
//! (когда окно свернуто в системный трей):
//! - Обнаружено обновление Antigravity (автопатч Language Server)
//! - Доступна новая версия Open Antigravity
//! - Результаты проверки сети

/// Отправляет системное уведомление.
pub fn send(title: &str, message: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .arg("-a")
            .arg("Open Antigravity")
            .arg(title)
            .arg(message)
            .spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "[reflection.assembly]::loadwithpartialname('System.Windows.Forms') | Out-Null; \
             $notify = new-object system.windows.forms.notifyicon; \
             $notify.icon = [system.drawing.systemicons]::information; \
             $notify.visible = $true; \
             $notify.showballoontip(5000, '{}', '{}', [system.windows.forms.tooltipicon]::None)",
            title.replace('\'', "''"),
            message.replace('\'', "''")
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_does_not_panic() {
        send("Open Antigravity Test", "Тестовое уведомление");
    }
}
