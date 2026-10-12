# Dev Archaeologist shell hook. Add this line to ~/.zshrc:
#   source ~/.dev-arch/dev-arch.zsh
# It sends the first word of each command, its exit code, the folder and the times to the
# local Dev Archaeologist app over a unix socket. Arguments never leave the shell.

zmodload zsh/datetime 2>/dev/null
autoload -Uz add-zsh-hook

__da_sock="$HOME/.dev-arch/shell.sock"

__da_preexec() {
  __da_cmd=${${(z)1}[1]}
  __da_t0=$EPOCHREALTIME
}

__da_json_escape() {
  local s=${1//\\/\\\\}
  print -rn -- ${s//\"/\\\"}
}

__da_precmd() {
  local code=$?
  [[ -z $__da_cmd || ! -S $__da_sock ]] && { unset __da_cmd; return }
  print -r -- "{\"cmd\":\"$(__da_json_escape $__da_cmd)\",\"exit\":$code,\"cwd\":\"$(__da_json_escape $PWD)\",\"t0\":$__da_t0,\"t1\":$EPOCHREALTIME}" \
    | nc -U -w 1 "$__da_sock" >/dev/null 2>&1 &!
  unset __da_cmd
}

add-zsh-hook preexec __da_preexec
add-zsh-hook precmd  __da_precmd
