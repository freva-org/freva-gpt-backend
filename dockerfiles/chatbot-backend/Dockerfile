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

# First install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN dnf install -y rust cargo

# Create virtual environment for app
RUN conda create --name env 
RUN conda init bash 
RUN echo "conda activate env" >> ~/.bashrc
# RUN source ~/.bashrc

# Install the python packages
RUN conda install -c conda-forge -n env numpy matplotlib netcdf4 cartopy contourpy pandas xarray

# Move over the source code
COPY src /app/src

# Also copy over the Cargo.toml file to be able to start the server
COPY Cargo.toml /app/Cargo.toml

# Set the working directory
WORKDIR /app

# Copy over the secrets file to have access to the openai account
COPY .env /app/.env

# Add another environment variable so the app can have a valid python instance
# ENV PYTHONHOME /opt/conda/envs/env/
ENV PYTHONUSERBASE /opt/conda/envs/env
# ENV PYTHONPLATLIBDIR lib
ENV PYTHONPATH=/opt/conda/envs/env/lib/python3.12/site-packages
ENV PATH /opt/conda/envs/env/bin/:$PATH
ENV LD_LIBRARY_PATH /opt/conda/envs/env/lib/:$PATH

# Crashes the build with info about the python environment that will be used. Debug only
# ENV PYO3_PRINT_CONFIG 1

# Run the Rust code in very verbose mode
ENTRYPOINT [ "cargo", "run", "--", "-vv"]
