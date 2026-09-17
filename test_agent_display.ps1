# Test agent-display functionality
$process = Start-Process -FilePath "C:\v4pro\target\debug\deepseek-tui.exe" -ArgumentList "agent-display", "test-agent" -NoNewWindow -RedirectStandardOutput "C:\v4pro\display_output.txt" -RedirectStandardError "C:\v4pro\display_error.txt" -PassThru

# Wait for 5 seconds
Start-Sleep -Seconds 5

# Terminate the process
if (!$process.HasExited) {
    $process.Kill()
}

Write-Host "Output:"
if (Test-Path "C:\v4pro\display_output.txt") {
    Get-Content "C:\v4pro\display_output.txt"
}
Write-Host "`nErrors:"
if (Test-Path "C:\v4pro\display_error.txt") {
    Get-Content "C:\v4pro\display_error.txt"
}