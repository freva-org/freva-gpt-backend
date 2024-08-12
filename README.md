# freva-GPT2-backend

This gitlab project is for the backend of the second version of the freva-GPT project. 

This is only the backend, because the current plan is to create REST-Like API for interacting with freva-GPT
so that the frontend can be seperated from the backend. 

The rewrite is largely because the old frontend library was hard to work with 
and because the code needs to switch from OpenAI's V2 to V1 to enable the usage of non-OpenAI LLMs.

When the backend is stable enough, I'll add a visualization of the API here.

For an example on how to use the API, check out the jupyter notebook `example.ipynb` in the top directory of this repo. 