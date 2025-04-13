package main

import (
	"context"
	"os"
	"os/signal"

	"github.com/rosedblabs/rosedb/v2"
	"github.com/urfave/cli/v2"

	"github.com/justjakka/reencoder/files"
)

func runCmd(cCtx *cli.Context) error {
	ctx, err := initArgs(cCtx)
	if err != nil {
		return err
	}

	options := rosedb.DefaultOptions
	options.DirPath = ctx.Value("dbfile").(string)
	db, err := rosedb.Open(options)
	if err != nil {
		return err
	}
	defer db.Close()

	ctx = context.WithValue(ctx, "database", db)

	ctx, stop := signal.NotifyContext(ctx, os.Interrupt)
	defer stop()

	counter := int64(0)

	ctx = context.WithValue(ctx, "counter", &counter)

	if err = files.IndexFlacs(ctx); err != nil {
		return err
	}

	if err = files.ReencodeFlacs(ctx); err != nil {
		return err
	}

	if err := db.Sync(); err != nil {
		return err
	}
	return nil
}

func Start() {
	app := &cli.App{
		Name:        "reencoder",
		Usage:       "reencodes and stores info",
		UsageText:   "reencoder /path/to/folder",
		Description: "indexes files, checks for encoder and reencodes",
		Args:        true,
		ArgsUsage:   "specify amusic links to download",
		Flags: []cli.Flag{
			&cli.PathFlag{
				Name:    "path",
				Usage:   "Path to folder with files to reencode",
				Value:   ".",
				Aliases: []string{"p"},
			},
			&cli.PathFlag{
				Name:    "database",
				Usage:   "Path to database",
				Aliases: []string{"d"},
			},
			&cli.StringSliceFlag{
				Name:    "flac",
				Usage:   "Flac arguments to use when reencoding, can be used multiple times",
				Aliases: []string{"a"},
			},
		},
		Action: runCmd,
	}
	if err := app.Run(os.Args); err != nil {
		panic(err.Error())
	}
}
