# Antigravity Usage Widget

Aplicación específica de Google Antigravity dentro del monorepo. Consulta las
cuotas de la cuenta mediante el comando oficial `agy --print /usage --output-format json`;
no accede directamente a las credenciales ni consume tokens.

Desde la raíz:

```powershell
agy
python .\apps\antigravity-usage-widget\main.py
.\scripts\build-release.ps1 -App antigravity -Version "1.0.0" -Installer
```

El widget muestra dinámicamente todos los grupos de modelos y ventanas devueltos
por Antigravity CLI, con sus tiempos de reinicio.

Consulta el [README raíz](../../README.md) para requisitos y solución de errores.
