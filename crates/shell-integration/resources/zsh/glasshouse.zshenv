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
    builtin printf '\e]7;file://%s%s\a' "${HOST}" "${PWD// /%20}"  # cwd
    builtin printf '\e]133;A\a'                    # prompt start
    # Mark end-of-prompt / start-of-input. Re-append if a theme rebuilt PS1.
    if [[ "$PS1" != *$'\e]133;B\a'* ]]; then
      PS1="${PS1}%{"$'\e]133;B\a'"%}"
    fi
  }

  _glasshouse_preexec() {
    _glasshouse_running=1
    builtin printf '\e]133;C\a'                    # output start
  }

  add-zsh-hook precmd  _glasshouse_precmd
  add-zsh-hook preexec _glasshouse_preexec
fi
