// Applied before the stylesheet loads so the window never flashes the default theme before
// switching to a saved one. Kept as an external, same-origin script (not inline) so the app's
// CSP can require `script-src 'self'` without an `'unsafe-inline'` carve-out.
(function () {
  try {
    var theme = localStorage.getItem("beacon:theme");
    if (theme) document.documentElement.setAttribute("data-theme", theme);
  } catch (_) {
    // Best-effort -- a locked-down webview can throw here; falls back to the default theme.
  }
})();
