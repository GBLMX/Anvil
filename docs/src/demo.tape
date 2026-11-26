Output docs/assets/anvil-demo.gif

Set Shell bash
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 16
Set Width 1200
Set Height 760
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 50ms

Hide
Type "cd $(git rev-parse --show-toplevel)"
Enter
Type "rm -rf /tmp/anvil-vhs && cp -R docs/demo/workspace /tmp/anvil-vhs && mkdir -p /tmp/anvil-vhs/.config /tmp/anvil-vhs/.state && cd /tmp/anvil-vhs && git init -q && git add . && git -c user.email=demo@example.com -c user.name=Demo commit -qm init"
Enter
Type "cd /tmp/anvil-vhs && env -u NO_COLOR -u ANVIL_SETTINGS TERM=xterm-256color COLORTERM=truecolor ANVIL_TRUECOLOR=1 ANVIL_THEME=tokyonight XDG_CONFIG_HOME=$PWD/.config XDG_STATE_HOME=$PWD/.state anvil --truecolor=always -w . Makefile"
Enter
Sleep 4s
Show
Sleep 1s

Type " f"
Sleep 600ms
Type "notes.txt"
Sleep 1s
Enter
Sleep 1s

Type "o"
Type "- edited for the demo"
Escape
Type ":w"
Enter
Sleep 1s

Type "/Workspace"
Enter
Sleep 600ms
Type "n"
Sleep 700ms

Type " e"
Sleep 1s


Type ":q"
Hide
