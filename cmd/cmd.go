package main

import (
	"fmt"
	"os"
	"os/signal"
	"time"

	"github.com/tidwall/buntdb"
	"github.com/urfave/cli/v2"
	"golang.org/x/sync/errgroup"
)

func runCmd(cCtx *cli.Context) error {
	ctx, err := initCmd(cCtx)
	if err != nil {
		return err
	}

	db, err := buntdb.Open(ctx.Value("database").(string))
	if err != nil {
		return err
	}
	defer db.Close()

	ctx, stop := signal.NotifyContext(ctx, os.Interrupt)
	defer stop()

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
	return cCtx.Err()
}

func start() {
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
