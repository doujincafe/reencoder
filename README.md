# reencoder

scans a specified folder and reencodes flacs if they needed to be reencoded

## installation

you can use cargo to install:
`cargo install flac-reencoder`

or clone the repo and build it yourself

to build statically just run the default build command:
`cargo build -r`

to dynamically link to libsqlite3 and libflac libs, use the following command:
`cargo build -r --no-default-features -F linked`

```
Usage: flac-reencoder [OPTIONS] [path]

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
