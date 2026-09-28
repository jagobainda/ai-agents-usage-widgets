# OpenCode Usage Widget

Aplicación específica de OpenCode Go dentro del monorepo. Lee en modo de solo
lectura la credencial `opencode-go` guardada por OpenCode y consulta las ventanas
de uso de la suscripción Go.

Desde la raíz:

```powershell
opencode
# En la TUI: /connect -> OpenCode Go
python .\apps\opencode-usage-widget\main.py
.\scripts\build-release.ps1 -App opencode -Version "1.0.0" -Installer
```

El widget muestra las ventanas móvil, semanal y mensual. El endpoint de Go no
incluye el saldo de créditos de OpenCode Zen ni el consumo de otros proveedores.

Consulta el [README raíz](../../README.md) para requisitos y solución de errores.
