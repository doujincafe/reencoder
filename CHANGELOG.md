# v0.3.0
* removed claxon dep in favor of flac_codec, same for metaflac
* cleaned up code a bit

# v0.2.6-fix
* remembered about changelog.md
* fixed incorrect encoded_by tag match
* minor code improvements and fixes

# v0.1.2
* added better bar incremental logic by passing it to threads
* added graceful shutdown (albeit its a bit slow)
* checks file if it exists before reencoding
* removes temporary file if it was left uncleaned from the previous session
