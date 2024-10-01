#!/bin/bash
# Not a real compose file, but a script to compose the (currently just one) image. 
podman run -d --name freva-gpt-2-backend-instance -p 8502:8502 -v /work:/work:ro -v ./logs:/app/logs -v ./threads:/app/threads -v ./target:/app/target -v ./testdata:/data/inputFiles -v ./python_pickles:/app/python_pickles -p 11434:11434 freva-gpt2-backend 
#          ^    	    ^				^           ^             ^                   ^                         ^                       ^                               ^                                      ^ 
# Runs the Image            | 				|           |             |                   |                         |                       |                               |                                      | 
#          |                | 				|           |             |                   |                         |                       |                               ^ Access to the persistant data for the|code_interpreter
#          ^ In detached mode to not close when the terminal window closes        |                   |                         |                       |                                                                      |
#                           | 				|           |             |                   |                         |                       |                                                                      ^ The image to run
#                           ^ Call the resulting container "freva-gpt-2-backend-instance              |                         |                       |
#							|           |             |                   |                         |                       |
#							^ Binding the port at internally 8502 to externally 8502                |                       |
#                                                                   |             |                   |                         |                       |
#								    ^ Access to the entire work partition so all project can be found by the freva python library
#                                                                                 |                   |                         |                       |
#                                                                                 ^ Logs are written to ./logs, even from inside; you don't have to enter the container to read them
#                                                                                                     |                         |                       |
#                                                                                                     ^ Same with threads       |                       |
#                                                                                                                               |                       |
#                                                                                                                               ^ Caches compilation between container starts
#                                                                                                                                                       |
#                                                                                                                                                       ^ Access to test datasets
