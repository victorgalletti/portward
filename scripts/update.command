#!/bin/bash
# Double-click in Finder to install the build prepared by scripts/build.sh.
D="$HOME/Library/Application Support/Portward/staged"
osascript -e "do shell script \"/bin/bash \" & quoted form of \"$D/install.sh\" & \" --from \" & quoted form of \"$D\" with administrator privileges with prompt \"O Portward quer instalar uma atualização.\"" && open -n /Applications/Portward.app
