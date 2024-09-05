# Pull from fedora because I could only get it to work.
FROM fedora:latest

# Install necessary dependencies
RUN dnf install -y curl bzip2

# Download and install Miniconda
RUN curl -o /tmp/miniconda.sh https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh && \
    bash /tmp/miniconda.sh -b -p /opt/conda && \
    rm /tmp/miniconda.sh

# Add Conda to the PATH
ENV PATH="/opt/conda/bin:$PATH"

# Ensure Conda is initialized properly
#RUN conda init bash

# First install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN dnf install -y rust cargo

# Create virtual environment for app
RUN conda create --name env 
RUN conda init bash 
RUN echo "conda activate env" >> ~/.bashrc
RUN source ~/.bashrc && conda activate env

# Install the python packages
RUN conda install -c conda-forge numpy matplotlib netcdf4 cartopy contourpy pandas xarray

# Move over the source code
COPY src /app/src

# Also copy over the Cargo.toml file to be able to start the server
COPY Cargo.toml /app/Cargo.toml

# Set the working directory
WORKDIR /app

# Copy over the secrets file to have access to the openai account
COPY .env /app/.env

# Add another environment variable so the app can have a valid python instance
# ENV PYTHONHOME /usr/conda
# ENV PYTHONUSERBASE /opt/conda
# ENV PYTHONPLATLIBDIR lib
# ENV PATH="$PATH:/usr/conda/lib:/usr/conda/bin"
ENV PYTHONPATH=/opt/conda/envs/env/lib/python3.12/site-packages
# ENV PYO3_PYTHON=/opt/conda/bin/python3.12 # might not be needed anymore?


# Run the Rust code in very verbose mode
ENTRYPOINT [ "cargo", "run", "--", "-vv"]
