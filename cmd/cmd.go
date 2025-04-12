package main

import (
	"context"
	"os"
	"os/signal"

	badger "github.com/dgraph-io/badger/v4"
	"github.com/urfave/cli/v2"

	"github.com/justjakka/reencoder/files"
)

func runCmd(cCtx *cli.Context) error {
	ctx, err := initArgs(cCtx)
	if err != nil {
		return err
	}

	db, err := badger.Open(files.Options(ctx.Value("dbfile").(string)))
	if err != nil {
		return err
	}
	defer db.Close()

	ctx = context.WithValue(ctx, "database", db)

	ctx, stop := signal.NotifyContext(ctx, os.Interrupt)
	defer stop()

	if err = files.IndexFlacs(ctx); err != nil {
		return err
	}

	if err = files.ReencodeFlacs(ctx); err != nil {
		return err
	}

	return nil

	/*
		test := new(errgroup.Group)
		test.SetLimit(3)
		for n := 1; n < 10; n++ {
			test.Go(func() error {
				select {
				case <-ctx.Done():
					return nil
				default:
					fmt.Println("test")
					time.Sleep(3 * time.Second)
					return nil
				}
			})
		}
		_ = test.Wait()
		fmt.Println("reached")
		return cCtx.Err() */
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
		},
		Action: runCmd,
	}
	if err := app.Run(os.Args); err != nil {
		panic(err.Error())
	}
}
