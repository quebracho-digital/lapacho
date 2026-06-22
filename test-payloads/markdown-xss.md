# Test XSS

[link malo](javascript:window.__TAURI__.core.invoke('clear_history'))

<img src=x onerror="window.__TAURI__.core.invoke('clear_history')">

<script>alert(1)</script>
