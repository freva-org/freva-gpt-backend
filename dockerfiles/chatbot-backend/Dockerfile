ARG BUILD_TARGET=without-crocodash
# Pull from fedora because alpine and ubuntu both have issues with the python environment or podman
FROM fedora:latest AS base

# ---------------------
# General installs
# ---------------------

# Install Rust
RUN dnf install -y rust cargo curl

# -------------------
# Conda setup
# -------------------

# Download and install Miniconda (architecture-aware)
RUN ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        CONDA_ARCH="x86_64"; \
    elif [ "$ARCH" = "aarch64" ]; then \
        CONDA_ARCH="aarch64"; \
    else \
        echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    curl -Lo /tmp/miniforge.sh https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-Linux-${CONDA_ARCH}.sh && \
    bash /tmp/miniforge.sh -b -p /opt/conda && \
    rm /tmp/miniforge.sh

# Add Conda to the PATH
ENV PATH="/opt/conda/bin:$PATH"

# Create virtual environment for app
RUN conda create --name env "python>=3.11.10,<3.12"
RUN conda init bash
# Adding the conda activate command to the bashrc file means that all subsequent shells will have the conda environment activated
RUN echo "conda activate env" >> ~/.bashrc

# -----------------
# Uv and conda installs
# -----------------

# Uv is a much faster package manager and installer, we'll use it over conda where possible to cut time.
RUN pip install uv
# These libraries cannot be installed by uv, so we use conda.
RUN conda install --yes -c conda-forge -n env pytorch cartopy scipy
# All packages that were previously installed with conda and can be installed with uv because they are on PYPI.
RUN source activate env && uv pip install freva-client>=2509.2.0 dill numpy matplotlib pandas xarray xesmf netcdf4 contourpy geopy aiohttp requests scikit-learn geopandas healpy easygems astropy

# Create app directory
RUN mkdir /app
WORKDIR /app

# ========================================
# Stage: with-crocodash
# ========================================
FROM base AS with-crocodash

# Install unzip for Crocodash
RUN dnf install -y unzip git && \
    dnf clean all

# Download the Bathymetry dataset (GEBCO 2024) for Crocodash
RUN curl -L https://www.bodc.ac.uk/data/open_download/gebco/gebco_2024/zip/ -o /app/gebco_2024.zip


# Install necessary dependencies for CrocoDash, not found on pypi.
RUN conda install --yes -c conda-forge -n env "esmpy>=8.6.1,<8.7.0"

# ----------------------
# Crocodash setup
# ----------------------

# set up CrocoDash and CESM
# set up git email and user name

RUN git config --global user.email "freva@dkrz.de"
RUN git config --global user.name "clint"
RUN git clone --recurse-submodules https://github.com/CROCODILE-CESM/CrocoDash.git
RUN source activate env && uv pip install -e CrocoDash/CrocoDash/rm6/ \
    -e CrocoDash/CrocoDash/visualCaseGen/external/ipyfilechooser/ \
    -e CrocoDash/CrocoDash/visualCaseGen/external/mom6_bathy/ \
    -e CrocoDash/CrocoDash/visualCaseGen/ \
    "myst_parser>=4.0.0,<4.1.0" \
    -e CrocoDash/
RUN source activate env && pytest CrocoDash/tests/test_installation.py
RUN git clone https://github.com/ESCOMP/CESM.git
RUN git -C CESM checkout cesm3_0_beta03
RUN source activate env && ./CESM/bin/git-fleximod -C /app/CESM/ update
ENV CIME_MACHINE="ubuntu-latest"
RUN mkdir -p /app/CrocoDash/inputs/gebco
RUN unzip /app/gebco_2024.zip -d /app/CrocoDash/inputs/gebco
RUN rm /app/gebco_2024.zip

# ========================================
# Stage: without-crocodash (default)
# ========================================
FROM base AS without-crocodash

# No CrocoDash setup - skip directly to preparations

# ========================================
# Final stage (common for both targets)
# ========================================
ARG BUILD_TARGET=without-crocodash
FROM ${BUILD_TARGET:-without-crocodash} AS final

# -------------------------
# Preparations for launch
# -------------------------

# Also copy over the Cargo.toml file to be able to start the server
COPY Cargo.toml /app/Cargo.toml

# Copy over the secrets file to have access to the openai account and so that the backend is configured correctly
# COPY .env /app/.env
# Also copy over the environemnt example file to have a reference
COPY .env.example /app/.env.example

# The litellm also needs to be copied over to the backend so that the list of chatbots can be extracted
COPY litellm_config.yaml /app/litellm_config.yaml

# Add another environment variable so the app can have a valid python instance
ENV PYTHONUSERBASE /opt/conda/envs/env
ENV PYTHONPATH=/opt/conda/envs/env/lib/python3.11/site-packages
ENV PATH /opt/conda/envs/env/bin/:$PATH
ENV LD_LIBRARY_PATH /opt/conda/envs/env/lib/:$PATH

# DEBUG: Crashes the build with info about the python environment that will be used.
# ENV PYO3_PRINT_CONFIG 1

# -------------
# Launch
# -------------

# Move over the source code; for quick builds, always put this last.
COPY src /app/src

# Helps for debugging
COPY test /app/test

# Run the Rust code in very verbose mode, printing trace information
RUN cargo build --release
ENTRYPOINT [ "/app/target/release/freva-gpt2-backend", "-vv"]
