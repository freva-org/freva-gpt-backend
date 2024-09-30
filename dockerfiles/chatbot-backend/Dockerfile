# Pull from fedora because alpine and ubuntu both have issues with the python environment or podman
FROM fedora:latest

# Install necessary dependencies for miniconda
RUN dnf install -y curl bzip2

# Download and install Miniconda
RUN curl -o /tmp/miniconda.sh https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-x86_64.sh && \
    bash /tmp/miniconda.sh -b -p /opt/conda && \
    rm /tmp/miniconda.sh

# Add Conda to the PATH
ENV PATH="/opt/conda/bin:$PATH"

# Install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN dnf install -y rust cargo

# Create virtual environment for app
RUN conda create --name env 
RUN conda init bash 
# Adding the conda activate command to the bashrc file means that all subsequent shells will have the conda environment activated
RUN echo "conda activate env" >> ~/.bashrc 

# Install the python packages into the environment
RUN conda install -c conda-forge -n env numpy matplotlib netcdf4 cartopy contourpy pandas xarray freva scipy anaconda::scikit-learn pytorch dill

# Also copy over the Cargo.toml file to be able to start the server
COPY Cargo.toml /app/Cargo.toml

# Set the working directory
WORKDIR /app

# Copy over the secrets file to have access to the openai account
COPY .env /app/.env

# Add another environment variable so the app can have a valid python instance
ENV PYTHONUSERBASE /opt/conda/envs/env
ENV PYTHONPATH=/opt/conda/envs/env/lib/python3.12/site-packages
ENV PATH /opt/conda/envs/env/bin/:$PATH
ENV LD_LIBRARY_PATH /opt/conda/envs/env/lib/:$PATH

#Freva requires a file to be located there, tecnically it needs to be a cert file, but this should work for now
RUN touch /opt/conda/envs/env/freva/freva.crt

# Move over the source code
COPY src /app/src

# DEBUG: Crashes the build with info about the python environment that will be used. 
# ENV PYO3_PRINT_CONFIG 1

# Run the Rust code in very verbose mode, printing trace information
ENTRYPOINT [ "cargo", "run", "--", "-vv"]
