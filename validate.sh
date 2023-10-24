#! /usr/bin/env bash
err=0

for f in `find . -wholename "./example/*.system"` 
do
    xmllint --noout --schema system.xsd $f
    if (($? != 0)); then
        err=$((err + 1))
    fi
done

if ((${err} > 0)); then
    echo "There were errors while parsing"
    exit 1
fi
