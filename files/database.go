package files

import (
	"context"
	"encoding/json"
	"time"

	badger "github.com/dgraph-io/badger/v4"
	"github.com/dgraph-io/badger/v4/options"
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

func (filedata *FileInfo) updateFile(database *badger.DB, hashsum []byte) error {
	return database.Update(func(txn *badger.Txn) error {
		encoded, err := json.Marshal(filedata)
		if err != nil {
			return err
		}
		return txn.Set(hashsum, encoded)
	})
}

func (filedata *FileInfo) IndexFile(ctx context.Context, hashsum []byte) error {
	database := ctx.Value("database").(*badger.DB)
	encoder := ctx.Value("encoder").(string)

	err := filedata.evaluateFile(database, hashsum, encoder)
	switch err {
	case FileMoved, ReencodeNotNeeded:
		filedata.Process = false
	case ReencodeNeeded, badger.ErrKeyNotFound:

	default:
		return err
	}
	if err := filedata.updateFile(database, hashsum); err != nil {
		return err
	}
	return nil
}

func ReencodeFlacs(ctx context.Context) error {
	db := ctx.Value("database").(*badger.DB)
	wg := new(errgroup.Group)
	wg.SetLimit(4)
	err := db.View(func(txn *badger.Txn) error {
		opts := badger.DefaultIteratorOptions
		opts.PrefetchSize = 10
		it := txn.NewIterator(opts)
		defer it.Close()

		for it.Rewind(); it.Valid(); it.Next() {
			/* item := it.Item()
			key := item.Key()
			info, err := decodeDbInfo(item)
			if err != nil {
				return err
			}
			if err := info.reencodeFile(ctx); err != nil {
				return err
			}
			if err := info.updateFile(db, key); err != nil {
				return err
			} */
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
				wg.Go(func() error {
					select {
					case <-ctx.Done():
						return nil
					default:
						if err := info.reencodeFile(ctx); err != nil {
							return err
						}
						if err := info.updateFile(db, key); err != nil {
							return err
						}
						return nil
					}
				})
			}
		}
		return nil
	})
	wg.Wait()
	return err
}

func Options(path string) badger.Options {
	return badger.Options{
		Dir:      path,
		ValueDir: path,

		MemTableSize:        64 << 20,
		BaseTableSize:       2 << 20,
		BaseLevelSize:       10 << 20,
		TableSizeMultiplier: 2,
		LevelSizeMultiplier: 10,
		MaxLevels:           7,
		NumGoroutines:       8,
		MetricsEnabled:      true,

		NumCompactors:           4, // Run at least 2 compactors. Zero-th compactor prioritizes L0.
		NumLevelZeroTables:      5,
		NumLevelZeroTablesStall: 15,
		NumMemtables:            5,
		BloomFalsePositive:      0.01,
		BlockSize:               4 * 1024,
		SyncWrites:              false,
		NumVersionsToKeep:       1,
		CompactL0OnClose:        false,
		VerifyValueChecksum:     false,
		Compression:             options.Snappy,
		BlockCacheSize:          256 << 20,
		IndexCacheSize:          0,

		// The following benchmarks were done on a 4 KB block size (default block size). The
		// compression is ratio supposed to increase with increasing compression level but since the
		// input for compression algorithm is small (4 KB), we don't get significant benefit at
		// level 3.
		// NOTE: The benchmarks are with DataDog ZSTD that requires CGO. Hence, no longer valid.
		// no_compression-16              10	 502848865 ns/op	 165.46 MB/s	-
		// zstd_compression/level_1-16     7	 739037966 ns/op	 112.58 MB/s	2.93
		// zstd_compression/level_3-16     7	 756950250 ns/op	 109.91 MB/s	2.72
		// zstd_compression/level_15-16    1	11135686219 ns/op	   7.47 MB/s	4.38
		// Benchmark code can be found in table/builder_test.go file
		ZSTDCompressionLevel: 1,

		// (2^30 - 1)*2 when mmapping < 2^31 - 1, max int32.
		// -1 so 2*ValueLogFileSize won't overflow on 32-bit systems.
		ValueLogFileSize: 1<<30 - 1,

		ValueLogMaxEntries: 1000000,

		VLogPercentile: 0.0,
		ValueThreshold: (1 << 20),

		Logger:                        nil,
		EncryptionKey:                 []byte{},
		EncryptionKeyRotationDuration: 10 * 24 * time.Hour, // Default 10 days.
		DetectConflicts:               true,
		NamespaceOffset:               -1,
	}
}
