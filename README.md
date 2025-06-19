# reencoder

scans a specified folder and reencodes flacs if they needed to be reencoded

currently this project uses dev-0.6 branch of [Symphonia](https://github.com/pdeljanov/Symphonia/tree/dev-0.6) crate with [some additional edits](https://github.com/pdeljanov/Symphonia/pull/387)
```
Usage: reencoder [OPTIONS] [path]

Arguments:
  [path]  Path for indexing/reencoding

Options:
      --doit               Actually reencode files
  -c, --clean              Clean and dedupe database
  -t, --threads <threads>  Set number of reencoding threads [default: 4]
  -d, --db <db>            Path to databse file
  -g, --generate <shell>   Generate shell completions [possible values: bash, elvish, fish, powershell, zsh]
  -h, --help               Print help
  -V, --version            Print version
```
