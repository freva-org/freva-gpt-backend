# Pull from Alpine image
FROM alpine

# First install Rust. As this tends to stay the same, we put it at the beginning of the Dockerfile, so it's cached.
RUN apk add --no-cache rust cargo

# Install python 3.12
RUN apk add --no-cache python3

# Copy over the requirements file and install the dependencies
COPY requirements.txt /python/requirements.txt
RUN pip install -r /python/requirements.txt

# Move over the source code
COPY ../../src /app/src

# Set the working directory
WORKDIR /app

# Run the Rust code in very verbose mode
ENTRYPOINT [ "cargo", "run", "--", "-vv"]

