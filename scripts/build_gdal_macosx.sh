
# install anaconda:
# > brew install --cask anaconda

# using anaconda install gdal and libgdal:
# > conda install -c conda-forge gdal libgdal

export RUSTFLAGS='-C link-args=-Wl,-rpath,'"$CONDA_PREFIX"'/lib'
export GDAL_HOME="$CONDA_PREFIX"
export GDAL_VERSION=$(gdal-config --version)
cargo build -F gdal,bindgen
