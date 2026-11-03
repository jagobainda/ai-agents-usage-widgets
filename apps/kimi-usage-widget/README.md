# Kimi Usage Widget

Aplicación específica de Kimi Code dentro del monorepo. Reutiliza la sesión OAuth
guardada por Kimi Code y consulta las mismas ventanas de uso que muestra `/usage`.

Desde la raíz:

```powershell
kimi login
python .\apps\kimi-usage-widget\main.py
.\scripts\build-release.ps1 -App kimi -Version "1.0.0" -Installer
```

El widget muestra la ventana de 5 horas, la semanal en planes que la incluyan y la
mensual compartida. Respeta `KIMI_CODE_HOME` y las regiones `.com` y `.ai`.

Consulta el [README raíz](../../README.md) para requisitos y solución de errores.
