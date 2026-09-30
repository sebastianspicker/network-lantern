// WebDriver's debug diagnostics print the inherited environment. Pass only
// desktop/runtime essentials so unrelated account credentials cannot enter logs.
const allowed = new Set([
  'PATH', 'HOME', 'USER', 'LOGNAME', 'TMPDIR', 'TMP', 'TEMP', 'SystemRoot',
  'SYSTEMROOT', 'WINDIR', 'USERPROFILE', 'APPDATA', 'LOCALAPPDATA', 'PROGRAMDATA',
  'PROGRAMFILES', 'ProgramFiles', 'ProgramFiles(x86)', 'COMSPEC', 'PATHEXT',
  'DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR',
  'DBUS_SESSION_BUS_ADDRESS', 'LANG', 'LC_ALL', 'LC_CTYPE', 'CI', 'NO_COLOR',
]);
export function nativeEnvironment(source) {
  return Object.fromEntries(Object.entries(source).filter(([key]) => allowed.has(key)));
}
