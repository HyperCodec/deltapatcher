set default-list := true

[windows]
set shell := ["powershell.exe"]

run-example example:
    cargo run --package example --bin {{example}} --all-features

# this recipe is mostly for CI/CD rather than human interaction.
[windows]
run-all-examples:
    Get-ChildItem example/src/bin -Filter *.rs | ForEach-Object { just run-example $_.BaseName }

[unix]
run-all-examples:
    for example in example/src/bin/*.rs; do just run-example $(basename $example .rs); done
