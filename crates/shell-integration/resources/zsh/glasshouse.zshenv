# Glasshouse zsh integration. Loaded because Glasshouse set ZDOTDIR to this directory.
# 1) Restore the user's ZDOTDIR so .zprofile/.zshrc/.zlogin load from their usual place.
if [[ -n "${GLASSHOUSE_ORIG_ZDOTDIR+x}" ]]; then
  ZDOTDIR="$GLASSHOUSE_ORIG_ZDOTDIR"
  unset GLASSHOUSE_ORIG_ZDOTDIR
else
  unset ZDOTDIR
fi

# 2) Source the user's real .zshenv.
if [[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]]; then
  builtin source "${ZDOTDIR:-$HOME}/.zshenv"
fi

# 3) Install hooks once, for interactive shells only.
if [[ -o interactive && -z "${_GLASSHOUSE_HOOKED-}" ]]; then
  typeset -g _GLASSHOUSE_HOOKED=1
  typeset -g _glasshouse_running=""
  builtin autoload -Uz add-zsh-hook

  _glasshouse_precmd() {
    local ret=$?
    if [[ -n "$_glasshouse_running" ]]; then
      builtin printf '\e]133;D;%s\a' "$ret"        # command finished, with exit status
    fi
    _glasshouse_running=""
    _glasshouse_url_path "$PWD"
    builtin printf '\e]7;file://%s%s\a' "${HOST}" "$_glasshouse_url"  # cwd
    builtin printf '\e]133;A\a'                    # prompt start
    # Mark end-of-prompt / start-of-input. Re-append if a theme rebuilt PS1.
    if [[ "$PS1" != *$'\e]133;B\a'* ]]; then
      PS1="${PS1}%{"$'\e]133;B\a'"%}"
    fi
  }

  # Percent-encodes $1 for a file:// URL path into _glasshouse_url: every byte outside the
  # unreserved set and '/' (so '#', '%', '?', spaces and control bytes survive the round trip).
  _glasshouse_url_path() {
    emulate -L zsh
    local LC_ALL=C s="$1" c
    local -i i
    typeset -g _glasshouse_url=""
    for (( i = 1; i <= ${#s}; i++ )); do
      c="${s[i]}"
      case "$c" in
        [A-Za-z0-9/._~-]) _glasshouse_url+="$c" ;;
        *) builtin printf -v c '%%%02X' "'$c"; _glasshouse_url+="$c" ;;
      esac
    done
  }

  _glasshouse_preexec() {
    _glasshouse_running=1
    builtin printf '\e]133;C\a'                    # output start
  }

  add-zsh-hook precmd  _glasshouse_precmd
  add-zsh-hook preexec _glasshouse_preexec
fi
