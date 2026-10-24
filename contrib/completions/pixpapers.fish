# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_pixpapers_global_optspecs
    string join \n h/help V/version
end

function __fish_pixpapers_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_pixpapers_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_pixpapers_using_subcommand
    set -l cmd (__fish_pixpapers_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c pixpapers -n "__fish_pixpapers_needs_command" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -s V -l version -d 'Print version'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "add" -d 'Add wallpaper(s) to the library (original stored optimized)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "list" -d 'List the library'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "remove" -d 'Remove a wallpaper (entry + variants)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "set" -d 'Apply a wallpaper now (starts the daemon if needed)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "pixel" -d 'Set the pixel look for one wallpaper: 8|16|32|64|128|normal'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "placement" -d 'Set placement for one wallpaper: fit|cover-gutter|cover-crop|stretch'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "bg" -d 'Set border/gutter color (#rrggbb) for one wallpaper'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "gutter" -d 'Set gutter width in px (cover-gutter) for one wallpaper'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "default" -d 'Change defaults for new wallpapers / unset entries'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "enable" -d 'Force pixelation on for everything (default)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "disable" -d 'Turn pixelation off (originals only, placement unchanged)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "cycle" -d 'Rotate to the next wallpaper (by N, default 1)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "status" -d 'Daemon status'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "export" -d 'Offline render to a PNG (no daemon needed): e.g. 1920x1080'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "optimize" -d 'Re-run original optimization over the library'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "daemon" -d 'Run the wallpaper daemon (foreground)'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "stop" -d 'Ask a running daemon to stop'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "completions" -d 'Emit a shell completion script'
complete -c pixpapers -n "__fish_pixpapers_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand add" -l title -r
complete -c pixpapers -n "__fish_pixpapers_using_subcommand add" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand list" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand remove" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand set" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand pixel" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand placement" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand bg" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand gutter" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and not __fish_seen_subcommand_from pixel placement help" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and not __fish_seen_subcommand_from pixel placement help" -f -a "pixel"
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and not __fish_seen_subcommand_from pixel placement help" -f -a "placement"
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and not __fish_seen_subcommand_from pixel placement help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and __fish_seen_subcommand_from pixel" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and __fish_seen_subcommand_from placement" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and __fish_seen_subcommand_from help" -f -a "pixel"
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and __fish_seen_subcommand_from help" -f -a "placement"
complete -c pixpapers -n "__fish_pixpapers_using_subcommand default; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand enable" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand disable" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand cycle" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand status" -l json
complete -c pixpapers -n "__fish_pixpapers_using_subcommand status" -l bench -d 'Live render timings, variant cache stats, RSS/fd (R8 proof)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand status" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand export" -l pixel -r
complete -c pixpapers -n "__fish_pixpapers_using_subcommand export" -l placement -r
complete -c pixpapers -n "__fish_pixpapers_using_subcommand export" -l bg -r
complete -c pixpapers -n "__fish_pixpapers_using_subcommand export" -l gutter -r
complete -c pixpapers -n "__fish_pixpapers_using_subcommand export" -s o -l out -r -F
complete -c pixpapers -n "__fish_pixpapers_using_subcommand export" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand optimize" -l all
complete -c pixpapers -n "__fish_pixpapers_using_subcommand optimize" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand daemon" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand stop" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand completions" -s h -l help -d 'Print help'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "add" -d 'Add wallpaper(s) to the library (original stored optimized)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "list" -d 'List the library'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "remove" -d 'Remove a wallpaper (entry + variants)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "set" -d 'Apply a wallpaper now (starts the daemon if needed)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "pixel" -d 'Set the pixel look for one wallpaper: 8|16|32|64|128|normal'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "placement" -d 'Set placement for one wallpaper: fit|cover-gutter|cover-crop|stretch'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "bg" -d 'Set border/gutter color (#rrggbb) for one wallpaper'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "gutter" -d 'Set gutter width in px (cover-gutter) for one wallpaper'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "default" -d 'Change defaults for new wallpapers / unset entries'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "enable" -d 'Force pixelation on for everything (default)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "disable" -d 'Turn pixelation off (originals only, placement unchanged)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "cycle" -d 'Rotate to the next wallpaper (by N, default 1)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "status" -d 'Daemon status'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "export" -d 'Offline render to a PNG (no daemon needed): e.g. 1920x1080'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "optimize" -d 'Re-run original optimization over the library'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "daemon" -d 'Run the wallpaper daemon (foreground)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "stop" -d 'Ask a running daemon to stop'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "completions" -d 'Emit a shell completion script'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and not __fish_seen_subcommand_from add list remove set pixel placement bg gutter default enable disable cycle status export optimize daemon stop completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and __fish_seen_subcommand_from default" -f -a "pixel"
complete -c pixpapers -n "__fish_pixpapers_using_subcommand help; and __fish_seen_subcommand_from default" -f -a "placement"
