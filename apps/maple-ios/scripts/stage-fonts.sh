#!/bin/sh
# Bundle licensed, tracked font inputs without conversion or network access.
set -eu
destination="$TARGET_BUILD_DIR/$UNLOCALIZED_RESOURCES_FOLDER_PATH"
mkdir -p "$destination"
install -m 644 "$SCRIPT_INPUT_FILE_0" "$destination/Array-Regular.otf"
install -m 644 "$SCRIPT_INPUT_FILE_1" "$destination/Manrope-VariableFont_wght.ttf"
install -m 644 "$SCRIPT_INPUT_FILE_2" "$destination/GeistMono-Regular.ttf"
