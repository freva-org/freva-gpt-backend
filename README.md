# freva-GPT2-backend

This gitlab project is for the backend of the second version of the freva-GPT project. 

This is only the backend, because the current plan is to create REST-Like API for interacting with freva-GPT
so that the frontend can be seperated from the backend. 

The rewrite is largely because the old frontend library was hard to work with 
and because the code needs to switch from OpenAI's V2 to V1 to enable the usage of non-OpenAI LLMs.

For an example on how to use the API, check out the jupyter notebook `example.ipynb` in the top directory of this repo. 

Additionally, the notebook I use for testing the versions (`testing.ipynb`) is also in this repo to document the usage of the API in developement. 

To run the backend, navigate to this folder so that this file is on the top level.
Then run `podman-build.sh` to build the project and `compose.sh` to launch it. 
This requires podman to be configured correctly and that no container with the name `freva-gpt2-backend-instance` is running. 
In that case, stop and remove that container and re-run `compose.sh`. 