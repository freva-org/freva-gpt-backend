# Pull from fedora because alpine and ubuntu both have issues with the python environment or podman
FROM fedora:latest

# Install necessary dependencies for miniconda
RUN dnf install -y curl bzip2

# Download and install Miniconda
RUN curl -Lo /tmp/miniforge.sh https://github.com/conda-forge/miniforge/releases/latest/download/Miniforge3-Linux-x86_64.sh && \
    bash /tmp/miniforge.sh -b -p /opt/conda && \
    rm /tmp/miniforge.sh

# Add Conda to the PATH
ENV PATH="/opt/conda/bin:$PATH"

# Install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN dnf install -y rust cargo

# Create virtual environment for app
RUN conda create --name env python=3.12
RUN conda init bash
# Adding the conda activate command to the bashrc file means that all subsequent shells will have the conda environment activated
RUN echo "conda activate env" >> ~/.bashrc

# Install the python packages into the environment. This is split up to improve caching.
RUN conda install --yes -c conda-forge -n env freva-client dill
RUN conda install --yes -c conda-forge -n env numpy matplotlib pandas xarray xesmf scipy
RUN conda install --yes -c conda-forge -n env netcdf4 cartopy contourpy geopy
RUN conda install --yes -c conda-forge -n env aiohttp requests
RUN conda install --yes -c conda-forge -n env scikit-learn pytorch geopandas healpy easygems astropy

# Also copy over the Cargo.toml file to be able to start the server
COPY Cargo.toml /app/Cargo.toml

# Set the working directory
WORKDIR /app

# Copy over the secrets file to have access to the openai account
COPY .env /app/.env

# The litellm also needs to be copied over to the backend so that the list of chatbots can be extracted
COPY litellm_config.yaml /app/litellm_config.yaml

# Add another environment variable so the app can have a valid python instance
ENV PYTHONUSERBASE /opt/conda/envs/env
ENV PYTHONPATH=/opt/conda/envs/env/lib/python3.12/site-packages
ENV PATH /opt/conda/envs/env/bin/:$PATH
ENV LD_LIBRARY_PATH /opt/conda/envs/env/lib/:$PATH

# Freva requires a file to be located there, tecnically it needs to be a cert file, but this should work for now
# RUN touch /opt/conda/envs/env/freva/freva.crt
# The new library doesn't require this anymore.

# DEBUG: Crashes the build with info about the python environment that will be used.
# ENV PYO3_PRINT_CONFIG 1

# Move over the source code; for quick builds, always put this last.
COPY src /app/src

# Run the Rust code in very verbose mode, printing trace information
ENTRYPOINT [ "cargo", "run", "-r", "--", "-vv"]
