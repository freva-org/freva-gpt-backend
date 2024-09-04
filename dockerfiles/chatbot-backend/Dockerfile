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

# Install conda dependiencies
# RUN dnf install -y bzip2

# Install python
# RUN dnf install -y python3 conda

# First create and activate the environment because conda needs it.
# RUN conda create -n env
# RUN conda init
# RUN conda activate env


# Create virtual environment for app
RUN conda create --name env 
RUN conda init bash && \
. /root/.bashrc && \
conda activate env

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

# Remove the standard installed python to force the app to choose the conda version
# RUN rm -r /usr/lib64/python3.12

# Add another environment variable so the app can have a valid python instance
# ENV PYTHONHOME /usr/conda
# ENV PYTHONPLATLIBDIR lib
# ENV PATH="$PATH:/usr/conda/lib:/usr/conda/bin"


# Run the Rust code in very verbose mode
ENTRYPOINT [ "cargo", "run", "--", "-vv"]
