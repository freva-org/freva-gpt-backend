# Pull from Alpine image
FROM alpine

# First install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN apk add --no-cache rust cargo

# Install python 3.12
RUN apk add --no-cache python3

# Prepare matplotlib install
RUN pip3 install --upgrade pip setuptools wheel py3-numpy
RUN apk add --no-cache --virtual .build-deps gcc g++ zlib-dev make python3-dev py-numpy-dev jpeg-dev
RUN pip3 install matplotlib
RUN apk del .build-deps

# Install other dependencies
RUN pip install netcdf4 cartopy contourpy pandas xarray

# # Copy over the requirements file and install the dependencies
# COPY requirements.txt /python/requirements.txt
# RUN pip install -r /python/requirements.txt

# Move over the source code
COPY ../../src /app/src

# Set the working directory
WORKDIR /app

# Run the Rust code in very verbose mode
ENTRYPOINT [ "cargo", "run", "--", "-vv"]

# DEBUG: Test, I need these pyhton packages:
#     pip install matplotlib
# pip install netcdf4 cartopy
# pip install contourpy
# pip install pandas
# pip install xarray