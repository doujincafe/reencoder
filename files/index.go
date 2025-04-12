package files

import (
	"context"
	"io/fs"
	"path/filepath"
	"time"

	"github.com/briandowns/spinner"
	"golang.org/x/sync/errgroup"
)

func IndexFlacs(ctx context.Context) error {
	spin := spinner.New(spinner.CharSets[9], 100*time.Millisecond)
	spin.Suffix = " Indexing flacs..."

	spin.Start()

	wg := new(errgroup.Group)
	wg.SetLimit(100)

	if err := filepath.WalkDir(ctx.Value("path").(string), func(path string, info fs.DirEntry, err error) error {
		select {
		case <-ctx.Done():
			spin.FinalMSG = "Stopping...\n"
			spin.Stop()
			return nil
		default:
			if err != nil {
				return err
			}
			if !info.IsDir() {
				if filepath.Ext(path) == ".flac" {
					wg.Go(func() error {
						select {
						case <-ctx.Done():
							return nil
						default:
							return ProcessFile(ctx, path, info)
						}
					})
				}

			}
			return nil
		}

	}); err != nil {
		return err
	}

	wg.Wait()
	spin.FinalMSG = "Done indexing flacs"
	spin.Stop()
	return nil
}
