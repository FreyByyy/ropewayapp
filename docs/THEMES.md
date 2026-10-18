# Файл темы Ropeway

Тему можно создать в приложении («Настройки → Оформление → Создать тему»),
сохранить в файл и отправить на другое устройство. Файл — это JSON
с расширением `.ropeway-theme`, не больше 512 КБ:

```json
{
  "format": "ropeway-theme",
  "version": 1,
  "id": "my-theme",
  "name": "Моя тема",
  "base": "fluent",
  "layout": "navview",
  "icons": "fluent",
  "modes": ["light", "dark"],
  "accent": "#0067c0",
  "colors": { "light": { "bg": "#f3f3f3" }, "dark": { "bg": "#202020" } },
  "radius": 8,
  "motion": 1,
  "mica": true,
  "css": "[data-mode=dark] .bubble { letter-spacing: .01em }"
}
```

| Поле | Значения |
|---|---|
| `base` | стиль элементов: `fluent`, `material`, `neu`, `glass`, `retro`, `ropeway` |
| `layout` | раскладка окна: `navview`, `rail`, `soft`, `sidebar` |
| `icons` | набор иконок: `fluent`, `material`, `puffy`, `glass`, `retro`, `ropeway` |
| `modes` | какие режимы есть у темы: `light`, `dark` или оба |
| `accent` | акцентный цвет |
| `colors` | цвета для каждого режима; каких нет, те выводятся из акцента |
| `radius` | скругление углов, пиксели |
| `motion` | множитель скорости анимаций |
| `mica` | эффект Mica на Windows 11 |
| `css` | свой CSS поверх стилей приложения |

В своём CSS на `<html>` доступны атрибуты `data-base`, `data-layout`,
`data-mode` и переменные `--accent`, `--bg`, `--panel`, `--text` и другие.
Перед установкой темы со своим CSS Ropeway предупреждает об этом.

Темы хранятся в папке данных приложения, в `themes/`.
