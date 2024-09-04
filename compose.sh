#!/bin/bash
# Not a real compose file, but a script to compose the (currently just one) image. 
podman run -d --name freva-gpt-2-backend-instance -p 8502:8502 -v /work/bm1159/XCES/data4xces:/work/bm1159/XCES/data4xces:ro -v ./logs:/app/logs -v ./threads:/app/threads -v ./target:/app/target freva-gpt2-backend 
# Runs the ^ image	    |				^           ^                                                     ^             ^
#          |                | 				|           |                                                     |             |
#          ^ In detached mode to not close when the terminal window closes                                                |             |
#                           | 				|           |                                                     |             |
#                           ^ Call the resulting container "freva-gpt-2-backend-instance"                                 |             |
#							|           |                                                     |             |
#							^ Binding the port at internally 8502 to externally 8502          |             |
#                                                                   |                                                     |             |
#								    ^ Giving access to the data4xces directory, read-only ^ mapping internally to the same directory
#                                                                                                                                       |
#                                                                                                                                       ^ On the image called freva-gppt2-backend
