# Glasshouse fish integration, auto-loaded via XDG_DATA_DIRS.
status is-interactive; or exit 0
set -q _GLASSHOUSE_HOOKED; and exit 0
set -g _GLASSHOUSE_HOOKED 1

# Remove our prefix from XDG_DATA_DIRS so child processes see the user's original value.
if set -q GLASSHOUSE_XDG_DATA_DIRS_PREFIX
    set -gx XDG_DATA_DIRS (string replace -- "$GLASSHOUSE_XDG_DATA_DIRS_PREFIX:" "" "$XDG_DATA_DIRS")
    set -e GLASSHOUSE_XDG_DATA_DIRS_PREFIX
end

# Wraps the user's fish_prompt so the prompt ends with OSC 133;B (input starts): fish has no
# hook between drawing the prompt and reading input. Checked before every prompt, because the
# prompt is autoloaded lazily and themes or fish_config can redefine it at any time.
function __glasshouse_wrap_prompt
    functions -q fish_prompt; or return
    functions fish_prompt | string match -q '*__glasshouse_user_prompt*'; and return
    functions -e __glasshouse_user_prompt
    functions -c fish_prompt __glasshouse_user_prompt
    function fish_prompt
        __glasshouse_user_prompt
        printf '\e]133;B\a'
    end
end

function __glasshouse_prompt --on-event fish_prompt
    # url style percent-encodes everything but unreserved characters and '/'.
    printf '\e]7;file://%s%s\a' (hostname) (string escape --style=url -- $PWD)
    printf '\e]133;A\a'
    __glasshouse_wrap_prompt
end

function __glasshouse_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end

function __glasshouse_postexec --on-event fish_postexec
    printf '\e]133;D;%s\a' $status
end
