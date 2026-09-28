# Claude Usage Widget

Aplicación específica de Claude dentro del monorepo. Reutiliza las credenciales
administradas por Claude Code y consulta su endpoint interno de uso.

Desde la raíz:

```powershell
python .\apps\claude-usage-widget\main.py
.\scripts\build-release.ps1 -App claude -Version "1.0.0" -Installer
```

Consulta el [README raíz](../../README.md) para requisitos y advertencias.
