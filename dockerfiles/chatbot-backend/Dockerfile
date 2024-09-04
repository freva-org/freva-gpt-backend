# Pull from Alpine image with miniconda3 installed
FROM frolvlad/alpine-miniconda3

# First install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN apk add --no-cache rust cargo

# Install python 3.12
RUN apk add --no-cache python3 

# Install the python packages
RUN conda install -c conda-forge numpy matplotlib netcdf4 cartopy contourpy pandas xarray




# Move over the source code
COPY ../../src /app/src

# Set the working directory
WORKDIR /app

# Run the Rust code in very verbose mode
ENTRYPOINT [ "cargo", "run", "--", "-vv"]
