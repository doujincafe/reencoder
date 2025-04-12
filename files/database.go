package files

import (
	"context"
	"encoding/json"

	badger "github.com/dgraph-io/badger/v4"
	progressbar "github.com/schollz/progressbar/v3"
	"golang.org/x/sync/errgroup"
)

func decodeDbInfo(item *badger.Item) (*FileInfo, error) {
	var info FileInfo
	if err := item.Value(func(val []byte) error {
		if err := json.Unmarshal(val, &info); err != nil {
			return err
		}
		return nil
	}); err != nil {
		return nil, err
	}
	return &info, nil
}

func getInfoFromDb(database *badger.DB, hashsum []byte) (*FileInfo, error) {
	var info *FileInfo
	if err := database.View(func(txn *badger.Txn) error {
		item, err := txn.Get(hashsum)
		if err != nil {
			return err
		}

		info, err = decodeDbInfo(item)
		if err != nil {
			return err
		}
		return nil
	}); err != nil {
		return nil, err
	}
	return info, nil
}

func (filedata *FileInfo) evaluateFile(database *badger.DB, hashsum []byte, encoder string) error {
	info, err := getInfoFromDb(database, hashsum)
	if err != nil {
		return err
	}

	if info.Process {
		return ReencodeNeeded
	}
	if filedata.Encoder != encoder {
		return ReencodeNeeded
	}
	if info.AbsPath != filedata.AbsPath {
		return FileMoved
	}
	return ReencodeNotNeeded
}

func (filedata *FileInfo) updateFile(wb *badger.WriteBatch, hashsum []byte) error {
	encoded, err := json.Marshal(filedata)
	if err != nil {
		return err
	}
	return wb.Set(hashsum, encoded)
}

func (filedata *FileInfo) IndexFile(ctx context.Context, hashsum []byte) error {
	database := ctx.Value("database").(*badger.DB)
	encoder := ctx.Value("encoder").(string)
	counter := ctx.Value("counter").(*int64)

	err := filedata.evaluateFile(database, hashsum, encoder)
	switch err {
	case FileMoved, ReencodeNotNeeded:
		filedata.Process = false
	case ReencodeNeeded, badger.ErrKeyNotFound:
		*counter += 1
	default:
		return err
	}

	wb := database.NewWriteBatch()
	defer wb.Cancel()
	if err := filedata.updateFile(wb, hashsum); err != nil {
		return err
	}
	if err := wb.Flush(); err != nil {
		return err
	}
	return nil
}

func ReencodeFlacs(ctx context.Context) error {
	db := ctx.Value("database").(*badger.DB)
	bar := progressbar.NewOptions64(
		*ctx.Value("counter").(*int64),
		progressbar.OptionSetDescription("Reencoding..."),
		progressbar.OptionShowCount(),
	)
	defer bar.Close()

	wb := db.NewWriteBatch()
	defer wb.Cancel()

	wg := new(errgroup.Group)
	wg.SetLimit(4)
	err := db.View(func(txn *badger.Txn) error {
		opts := badger.DefaultIteratorOptions
		opts.PrefetchSize = 10
		it := txn.NewIterator(opts)
		defer it.Close()

		for it.Rewind(); it.Valid(); it.Next() {
			select {
			case <-ctx.Done():
				return nil
			default:
				item := it.Item()
				key := item.Key()
				info, err := decodeDbInfo(item)
				if err != nil {
					return err
				}
				if info.Process {
					wg.Go(func() error {
						select {
						case <-ctx.Done():
							return nil
						default:
							if err := info.reencodeFile(ctx); err != nil {
								return err
							}
							if err := info.updateFile(wb, key); err != nil {
								return err
							}
							bar.Add64(1)
							return nil
						}
					})
				}

			}
		}
		return nil
	})
	wg.Wait()
	return err
}
