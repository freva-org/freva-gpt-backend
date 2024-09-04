# Pull from Alpine image
FROM alpine

# First install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN apk add --no-cache rust cargo

# Install python 3.12
RUN apk add --no-cache python3 py3-pip python3-dev

# Prepare matplotlib install
RUN pip3 install --upgrade --break-system-packages pip setuptools wheel numpy
RUN apk add --no-cache --virtual .build-deps gcc g++ zlib-dev make python3-dev py3-numpy-dev jpeg-dev
RUN pip3 install --break-system-packages matplotlib
RUN apk del .build-deps

# Netcdf4 needs hdf5, so we'll install it with apk
RUN apk add hdf5-dev netcdf-dev proj

# Cartopy also needs the proj to be configured with an environment variable
ENV PROJ_DIR=/usr/local
ENV PROJ_INCDIR=/usr/local/include

# Install other dependencies
RUN pip3 install --break-system-packages netcdf4 
RUN pip3 install --break-system-packages cartopy
RUN pip3 install --break-system-packages contourpy
RUN pip3 install --break-system-packages pandas
RUN pip3 install --break-system-packages xarray







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
