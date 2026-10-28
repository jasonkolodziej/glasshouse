# Glasshouse fish integration, auto-loaded via XDG_DATA_DIRS.
status is-interactive; or exit 0
set -q _GLASSHOUSE_HOOKED; and exit 0
set -g _GLASSHOUSE_HOOKED 1

# Remove our prefix from XDG_DATA_DIRS so child processes see the user's original value.
if set -q GLASSHOUSE_XDG_DATA_DIRS_PREFIX
    set -gx XDG_DATA_DIRS (string replace -- "$GLASSHOUSE_XDG_DATA_DIRS_PREFIX:" "" "$XDG_DATA_DIRS")
    set -e GLASSHOUSE_XDG_DATA_DIRS_PREFIX
end

function __glasshouse_prompt --on-event fish_prompt
    printf '\e]7;file://%s%s\a' (hostname) (string replace -a ' ' '%20' $PWD)
    printf '\e]133;A\a'
end

function __glasshouse_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end

function __glasshouse_postexec --on-event fish_postexec
    printf '\e]133;D;%s\a' $status
end
