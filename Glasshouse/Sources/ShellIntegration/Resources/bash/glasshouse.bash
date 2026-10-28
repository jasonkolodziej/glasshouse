# Glasshouse bash integration, loaded with `bash --rcfile <this> -i`.
# Emulate a login shell's startup files, then the usual interactive rc.
if [ -f /etc/profile ]; then . /etc/profile; fi
_glasshouse_profile=""
for f in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -f "$f" ]; then . "$f"; _glasshouse_profile=1; break; fi
done
# A real login shell reads only the profile (which conventionally sources .bashrc itself).
# With no profile at all, fall back to .bashrc so interactive settings still load.
if [ -z "$_glasshouse_profile" ] && [ -f "$HOME/.bashrc" ]; then . "$HOME/.bashrc"; fi
unset _glasshouse_profile

_glasshouse_running=""
_glasshouse_ready=""      # set after the first prompt, so rc-file commands don't count as user commands
_glasshouse_in_prompt=""  # set while PROMPT_COMMAND runs, so the user's own prompt hooks don't count
_glasshouse_status=0

_glasshouse_preexec() {
  # DEBUG fires before every simple command, including PROMPT_COMMAND and completion functions.
  [ -z "$_glasshouse_ready" ] && return
  [ -n "$_glasshouse_in_prompt" ] && return
  [ "$BASH_COMMAND" = "_glasshouse_prompt_begin" ] && return
  [ -n "$COMP_LINE" ] && return
  if [ -z "$_glasshouse_running" ]; then
    _glasshouse_running=1
    printf '\e]133;C\a'
  fi
}
_glasshouse_prompt_begin() {
  _glasshouse_status=$?   # must run first in PROMPT_COMMAND to see the real exit status
  _glasshouse_in_prompt=1
}
_glasshouse_prompt_end() {
  if [ -n "$_glasshouse_running" ]; then printf '\e]133;D;%s\a' "$_glasshouse_status"; fi
  _glasshouse_running=""
  printf '\e]7;file://%s%s\a' "$HOSTNAME" "${PWD// /%20}"
  printf '\e]133;A\a'
  _glasshouse_ready=1
  _glasshouse_in_prompt=""
}
trap '_glasshouse_preexec' DEBUG
# Works on macOS's bash 3.2 (no PS0, no PROMPT_COMMAND arrays).
PROMPT_COMMAND="_glasshouse_prompt_begin; ${PROMPT_COMMAND:+$PROMPT_COMMAND; }_glasshouse_prompt_end"
case "$PS1" in *$'\e]133;B\a'*) ;; *) PS1="${PS1}\[\e]133;B\a\]";; esac
