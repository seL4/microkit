#/bin/sh

cd ..
git clone https://github.com/seL4/capdl.git
cd -
cp capdl_pip.txt ../capdl/python-capdl-tool/setup.py
./pyenv/bin/python3.9 -m pip install ../capdl/python-capdl-tool/
