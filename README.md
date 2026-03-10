## What is this?

A command-line tool that encodes images to below a specified file size.

The source is published for what it's worth, but this is a completely personal-use tool I threw together after frequently hitting size limits when trying to post 4K gaming screenshots to mixi2.

## How it works

It tries the following options in order, and writes out the file as soon as the result falls below the specified size (default: 15 MiB; overridable with the `-s` option).
The output filename is `{{original filename}}-reenc.{{extension}}`.
If none of the options fit within the size limit, it returns an error.

- PNG, high compression settings
- JPEG, q = 100
- JPEG, q = 90

For 4K gaming screenshots, JPEG should almost always bring them under 15 MiB without issue.

By default, if the output file already exists, it returns an error. Use the `-f` option to force overwrite.
