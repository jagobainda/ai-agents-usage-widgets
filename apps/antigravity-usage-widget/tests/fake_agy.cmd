@echo off
setlocal

if "%~1"=="sample" (
    type "%~dp0fixtures\sample_usage.json"
    exit /b 0
)

if "%~1"=="noisy-sample" (
    echo I1120 startup
    echo {"conversation_id":"","status":"SUCCESS","duration_seconds":0,"num_turns":0,"usage":{"total_tokens":0},"command":{"name":"usage","data":{"groups":[{"name":"Gemini Models","buckets":[{"id":"gemini-weekly","name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":0.9925,"reset_time":"2026-11-27T14:32:20Z"},{"id":"gemini-5h","name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":0.9737,"reset_time":"2026-11-20T19:32:20Z"}]},{"name":"Claude and GPT models","buckets":[{"id":"3p-weekly","name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":1,"reset_time":"2026-11-27T14:57:01Z"},{"id":"3p-5h","name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":0.4,"reset_time":"2026-11-20T19:57:01Z"}]},{"name":"Gemini 4 Argon","buckets":[{"id":"gemini-argon-wtu-weekly","name":"Gemini 4 Argon Weekly Limit","window":"weekly","remaining_fraction":1,"reset_time":"2026-11-27T14:57:01Z"}]}]}}}
    exit /b 0
)

if "%~1"=="env" (
    echo {"status":"SUCCESS","value":"%AGY_CLI_DISABLE_AUTO_UPDATE%"}
    exit /b 0
)

if "%~1"=="sleep" (
    "%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 30"
    exit /b 0
)

if "%~1"=="auth-error" (
    >&2 echo Authentication required
    exit /b 1
)

if "%~1"=="status-error" (
    echo {"status":"ERROR","error":"rate limit exceeded"}
    exit /b 0
)

if "%~1"=="orphan-child" (
    start "" /b "%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 30"
    type "%~dp0fixtures\sample_usage.json"
    exit /b 0
)

exit /b 1
