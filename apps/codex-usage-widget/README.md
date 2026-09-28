# Codex Usage Widget

Aplicación específica de Codex dentro del monorepo. Obtiene límites de cuenta
mediante el protocolo oficial `codex app-server`; nunca accede directamente al
archivo `~/.codex/auth.json`.

Desde la raíz:

```powershell
codex login
python .\apps\codex-usage-widget\main.py
.\scripts\build-release.ps1 -App codex -Version "1.0.0" -Installer
```

Consulta el [README raíz](../../README.md) para requisitos y solución de errores.
