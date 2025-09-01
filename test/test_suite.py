# ---- Setup ----
import requests
import json
import pytest
from dataclasses import dataclass, field
from dotenv import load_dotenv
import os

base_url = "http://localhost:8502/api/chatbot"

load_dotenv()
auth_key = os.getenv("AUTH_KEY")
global_user_id = "testing"
auth_string = "&auth_key=" + auth_key + "&user_id=" + global_user_id # Only for testing
# In Version 1.6.1, the freva_config also needs to be set to a specific path. We won't be using this for now.
auth_string = auth_string + "&freva_config=" + "Cargo.toml" # Dummy value

# Starting with Version 1.10.0, a vault url needs to be supplied in the headers.
vault_url = "http://127.0.0.1:5001" # TODO: This might not work because of the local/docker devide.
rest_url = "http://localhost:5001" # This is the URL of the mock authentication server. 
headers = {
    "x-freva-vault-url": vault_url,
    "x-freva-rest-url": rest_url,
    "x-freva-user-token": "Bearer THE_MOCK_AUTH_WILL_JUST_ALLOW_ANYTHING_SO_THIS_JUST_HAS_TO_BE_HERE",
}


# ======================================
# ---- Helper Functions and Classes ----
# ======================================

def get_request(url, stream=False):
    return requests.get(base_url + url + auth_string, stream=stream, headers=headers)

def get_avail_chatbots():
    response = get_request("/availablechatbots?")
    print(response.text)
    return response.json()

def get_user_threads(num_threads=None):
    response = get_request("/getuserthreads?" + (f"&num_threads={num_threads}" if num_threads else ""))
    print(response.text)
    return response.json()

@dataclass
class StreamResult:
    chatbot: str | None
    raw_response: list = field(default_factory=list)
    json_response: list = field(default_factory=list)
    code_variants: list = field(default_factory=list)
    codeoutput_variants: list = field(default_factory=list)
    assistant_variants: list  = field(default_factory=list)
    image_variants: list = field(default_factory=list)
    server_hint_variants: list  = field(default_factory=list)
    thread_id: str | None = None

    def extract_variants(self):
        if self.json_response:
            # The stream can stream multiple Assistant or Code fragments one after the other, in order to get good UX, but that means that multiple fragments that form a single variant can be streamed one after the other.
            # So, for convenience, we'll combine consecutive fragments that form a single variant into a single variant, if that variant is Assistant or Code. 

            running_code = None # None or tuple of (code, code_id) (which is the content of the fragment)
            running_assistant = None # None or string (which is the content of the fragment)
            for fragment in self.json_response:
                variant = fragment["variant"]
                content = fragment["content"]

                if variant != "Code" and running_code:
                    self.code_variants.append(running_code)
                    running_code = None
                if variant != "Assistant" and running_assistant:
                    self.assistant_variants.append(running_assistant)
                    running_assistant = None

                if variant == "Code":
                    if running_code:
                        running_code = (running_code[0] + content[0], running_code[1])
                    else:
                        running_code = (content[0], content[1])
                elif variant == "Assistant":
                    if running_assistant:
                        running_assistant = running_assistant + content
                    else:
                        running_assistant = content
                elif variant == "CodeOutput":
                    self.codeoutput_variants.append(content[0])
                elif variant == "Image":
                    self.image_variants.append(content)
                elif variant == "ServerHint":
                    self.server_hint_variants.append(content)



            self.thread_id = json.loads(self.json_response[0]["content"])["thread_id"]
            print("Debug: thread_id: " + self.thread_id) # Alway print the thread_id for debugging, so that when a test fails, we know which thread_id to look at.

    def has_error_variants(self):
        return any([ "error" in i["variant"].lower() for i in self.json_response])

def generate_full_response(user_input, chatbot=None, thread_id=None, user_id=None) -> StreamResult:
    inner_url = "/streamresponse?input=" + user_input
    if chatbot:
        inner_url = inner_url + "&chatbot=" + chatbot
    if thread_id:
        inner_url = inner_url + "&thread_id=" + thread_id
        
    # The response is streamed, but we will consume it here and store it
    result = StreamResult(chatbot)
    response = get_request(inner_url, stream=True)
    
    # unassembled_response = [] # Because the response may not necessary be chunked correctly. We will assemble it here.
    # for delta in response:
    #     if delta.decode("utf-8")[0] == "{":
    #         unassembled_response.append(delta.decode("utf-8"))
    #     else:
    #         unassembled_response[-1] += delta.decode("utf-8")
    
    # # It's assembled now
    # result.raw_response = unassembled_response
    # result.json_response = [json.loads(i) for i in unassembled_response]

    # Because the python request library is highly unreliable when it comes to streaming, we will manually assemble the response packet by packet here. 
    raw_response = []
    reconstructed_packets = []
    buffer = ""
    for delta in response:
        # print(delta) # Debugging
        data = delta.decode("utf-8")
        buffer += data
        raw_response.append(data)

        # Each packet is a valid JSON object, so we try to parse the buffer until we get a successful parse.
        # Each packet must end at a }, so we will only consider the buffer from the start to each }.

        packet_found = True
        while packet_found:
            packet_found = False            
            closing_brace_locations = [i for i in range(len(buffer)) if buffer[i] == "}"]

            for closing_brace_location in closing_brace_locations:
                # Try to parse the buffer up to the closing brace location
                try:
                    packet = json.loads(buffer[:closing_brace_location + 1])
                    reconstructed_packets.append(packet)
                    buffer = buffer[closing_brace_location + 1:]
                    packet_found = True
                except json.JSONDecodeError:
                    # If we get a JSONDecodeError, we will just ignore it and continue
                    pass
        
        # All packets that we could parse are now in reconstructed_packets, and the buffer contains the rest of the data.
    result.raw_response = raw_response
    result.json_response = reconstructed_packets
    
    result.extract_variants()
    
    # Print the response for debugging, so that when a test fails, we know what the response was.
    print("Debug: Assistant variants: ")
    print(result.assistant_variants)
    print("Debug: Code variants: ")
    print(result.code_variants)
    print("Debug: CodeOutput variants: ")
    print(result.codeoutput_variants)
    # print("Debug: full json_response: ") # Disabled, too noisy
    # print(result.json_response)
    assert not result.has_error_variants(), "Error variants found in response!"
    return result

def get_thread_by_id(thread_id):
    reponse = get_request("/getthread?thread_id=" + thread_id)
    print(reponse.text)
    return reponse.json()

# ===========================
# ---- Testing functions ----
# ===========================

def test_is_up():
    get_request("/ping")
    get_request("/docs")
    

def print_help():
    response = get_request("/help") # Same as /ping
    print(response.text)

def print_docs():
    response = get_request("/docs")
    print(response.text)


def test_available_chatbots():
    response = get_avail_chatbots()
    assert "gpt-5" in response
    assert "gpt-5-mini" in response
    assert "gpt-4o-mini" in response
    assert "gpt-4o" in response


def get_hello_world_thread_id() -> str:
    response = generate_full_response("Please use the code_interpreter tool to run the following code exactly and only once: \"print('Hello\\nWorld\\n!', flush=True)\".", chatbot="gpt-4.1-mini")
    # Just make sure the code output contains "Hello World !"
    assert any("Hello\nWorld\n!" in i for i in response.codeoutput_variants)
    # Now return the thread_id for further testing
    return response.thread_id

def test_hello_world():
    ''' Does the printing of Hello World work? '''
    thread_id = get_hello_world_thread_id()
    # Now use the thread_id to test the getthread endpoint
    hw_thread = get_thread_by_id(thread_id) # Type: list of variants.
    temp = StreamResult(None)
    temp.json_response = hw_thread
    temp.extract_variants()
    assert temp.thread_id == thread_id # Just make sure the thread_id is correct
    assert any("Hello\nWorld\n!" in i for i in temp.codeoutput_variants) # Make sure the code output contains "Hello World !"


def test_sine_wave(display = False):
    ''' Can the code_interpreter tool handle matplotlib and output an image? ''' # Base functionality test
    response = generate_full_response("This is a test regarding your capabilities of using the code_interpreter tool and whether it supports matplotlib. Please use the code_interpreter tool to run the following code: \"import numpy as np\nimport matplotlib.pyplot as plt\nt = np.linspace(-2 * np.pi, 2 * np.pi, 100)\nsine_wave = np.sin(t)\nplt.figure(figsize=(10, 5))\nplt.plot(t, sine_wave, label='Sine Wave')\nplt.title('Sine Wave from -2π to 2π')\nplt.xlabel('Angle (radians)')\nplt.ylabel('Sine value')\nplt.axhline(0, color='black', linewidth=0.5, linestyle='--')\nplt.axvline(0, color='black', linewidth=0.5, linestyle='--')\nplt.grid()\nplt.legend()\nplt.show()\".", chatbot="gpt-4.1-mini")
    # We want to make sure we have generated code, code output and an image. But we want to print the assistant response if it fails.
    print(response.assistant_variants)
    assert response.code_variants
    assert response.codeoutput_variants
    assert response.image_variants

    # Only possible in a notebook
    # if display: # For manual testing, ipytest won't display the image
    #     from IPython.display import display, Image
    #     from base64 import b64decode
    #     for image in response.image_variants:
    #         display(Image(data=b64decode(image), format='png'))


def test_persistent_thread_storage():
    ''' Does the backend remember the content of a thread? ''' # Base functionality test
    response = generate_full_response("Please add 2+2 in the code_interpreter tool.", chatbot="gpt-4.1-mini")
    # Now follow up with another request to the same thread_id, to test whether the storage is persistent
    response2 = generate_full_response("Now please multiply the result by 3.", chatbot="gpt-4.1-mini", thread_id=response.thread_id)
    # The code output should now contain 12
    assert any("12" in i for i in response2.codeoutput_variants)


def test_persistant_state_storage():
    ''' Can the backend refer to the same variable in different tool calls? ''' # Since Version 1.6.3
    # Here, we want to test whether the value of a variable is stored between tool calls (not requests)
    response = generate_full_response("Please assign the value 42 to the variable x in the code_interpreter tool. After that, call the tool with the code \"print(x, flush=True)\", without assigning x again. It's a test for the presistance of data.", chatbot="gpt-4.1-mini")
    # The code output should now contain 42
    assert any("42" in i for i in response.codeoutput_variants)
    # Also make sure there are actually two code variants
    assert len(response.code_variants) == 2


def test_persistant_xarray_storage():
    ''' Can the backend refer to the same xarray in different tool calls? ''' # Since Version 1.6.5
    reponse = generate_full_response("Please generate a simple xarray dataset in the code_interpreter tool and print out the content. After that, call the tool with the code \"print(ds, flush=True)\", without generating the dataset again. It's a test for the presistance of data, specifically whether xarray Datasets also work.", chatbot="gpt-4.1-mini")
    # The code output should now contain the content of the xarray dataset
    assert any(("xarray.Dataset" in i or "xarray.DataArray" in i) for i in reponse.codeoutput_variants)
    # Also make sure there are actually two code variants
    assert len(reponse.code_variants) == 2


def test_models_available():
    ''' Can the backend use the common models qwen2.5:3b, o4-mini and gpt-5-nano? ''' # Since Version 1.7.1, 1.10.1 and 1.10.2 respectively. 
    qwen_response = generate_full_response("This is a test request for your basic functionality. Please respond with (200 Ok) and exit. Don't use the code interpreter, just say it.", chatbot="qwen2.5:3b")
    # The assistant output should now contain "200 Ok"
    assert any("200 ok" in i.lower() for i in qwen_response.assistant_variants)

    o4_mini_response = generate_full_response("This is a test request for your basic functionality. Please respond with (200 Ok) and exit. Don't use the code interpreter, just say it.", chatbot="o4-mini")
    # The assistant output should now contain "200 Ok"
    assert any("200 ok" in i.lower() for i in o4_mini_response.assistant_variants)

    gpt_5_nano_response = generate_full_response("This is a test request for your basic functionality. Please respond with (200 Ok) and exit. Don't use the code interpreter, just say it.", chatbot="gpt-5-nano")
    # The assistant output should now contain "200 Ok"
    assert any("200 ok" in i.lower() for i in gpt_5_nano_response.assistant_variants)


def test_qwen_code_interpreter():
    ''' Can the backend get a code response from Qwen? ''' # Since Version 1.7.1
    response = generate_full_response("Please use the code_interpreter tool to run `print(2938429834 * 234987234)`. Make sure to adhere to the JSON format!", chatbot="qwen2.5:3b")
    # The code output should now contain the result of the multiplication
    assert any("690493498994739156" in i for i in response.codeoutput_variants)

def test_heartbeat():
    ''' Can the backend send a heartbeat while a long calculation is running? ''' # Since Version 1.8.1
    response = generate_full_response("Please use the code_interpreter tool to run the following code: \"import time\ntime.sleep(7)\".", chatbot="gpt-4.1-mini")
    # There should now, in total be at least three ServerHint Variants
    assert len(response.server_hint_variants) >= 3
    # The second Serverhint (first is thread_id) should be JSON containing "memory", "total_memory", "cpu_last_minute", "process_cpu" and "process_memory"
    first_hearbeat = json.loads(response.server_hint_variants[1])
    assert "memory" in first_hearbeat
    assert "total_memory" in first_hearbeat
    assert "cpu_last_minute" in first_hearbeat
    assert "process_cpu" in first_hearbeat
    assert "process_memory" in first_hearbeat


# TODO: implement 1.8.3 feature of stopping a tool call! (and the 1.8.9 feature that derives from it)


def test_syntax_hinting():
    ''' Can the backend provide extended hints on syntax errors? ''' # Since Version 1.8.4
    response = generate_full_response("Please use the code_interpreter tool to run the following code: \"print('Hello World'\". This is a test for the improved syntax error reporting. If a hint containing the syntax error is returned, the test is successful.", chatbot="gpt-4.1-mini")
    # We can now check the Code Output for the string "Hint: the error occured on line", as well as "SyntaxError"
    assert any("Hint: the error occured on line" in i for i in response.codeoutput_variants)
    assert any("SyntaxError" in i for i in response.codeoutput_variants)

def test_regression_variable_storage():
    ''' Does the backend correctly handle the edge case of variable storage? ''' # Since Version 1.8.9
    input = "This is a test on a corner case of the code_interpreter tool: variables don't seem to be stored if the code errors before the last line.\
To test this. Please run the following code: \"x = 42\nraise Exception('This is a test exception')\nprint('Padding for last-line-logic')\","
    response = generate_full_response(input, chatbot="gpt-4.1-mini")
    # The code output should now contain the exception message
    assert any(["This is a test exception" in i for i in response.codeoutput_variants])

    # Now make sure the variable x is still stored
    response2 = generate_full_response("Please demonstrate the fact that the code interpreter does not persist variables after exceptions by printing x without reassigning it.", chatbot="gpt-5-mini", thread_id=response.thread_id)
    # The code output should now contain 42
    assert any(["42" in i for i in response2.codeoutput_variants])

def test_third_request():
    '''Can the backend store information the user gave over multiple requests?''' # Since Version 1.8.14
    # The test is for a regression that happened when the backend moved to mongodb from storing threads on disk.
    # Basically, I forgot to append the existing thread, so the conten was just overwritten.
    # This lead to the chatbot not being able to recall what the user wrote in their first request, once they do a third request, hence the name.
    
    # response1 = generate_full_respone("Please remember the following information: \"I am a software engineer and I like to play chess\".", chatbot="gpt-4o-mini")
    # This doesn't work well because it technically does work, but is not in the style of what frevaGPT was designed to work with.
    response1 = generate_full_response("Hi! I'm Sebastian from the DRKZ. Who are you?", chatbot="gpt-4.1-mini")
    # The assistant should now remember the users name.
    response2 = generate_full_response("Nice to meet you! What do you think about chess?", chatbot="gpt-4.1-mini", thread_id=response1.thread_id) # Just some filler. I'm not good at small talk.
    
    response3 = generate_full_response("What was my name again?", chatbot="gpt-4.1-mini", thread_id=response1.thread_id)
    
    assert any("Sebastian" in i for i in response3.assistant_variants)


should_test_mongo = True
def test_get_user_threads():
    ''' Can the Frontend request the threads of a user? ''' # Since Version 1.9.0
    # Version 1.9.0 introduced the ability to request the threads of a user.
    # This requires MongoDB to be turned on, so this switch can be turned off to disable this feature.
    if should_test_mongo:
        response = get_user_threads()
        # The response should be a list of threads, each with a thread_id and a chatbot name
        assert isinstance(response, list)
        assert all(isinstance(i, dict) for i in response)
        assert all("thread_id" in i for i in response)
        assert all("user_id" in i for i in response)
        assert all("date" in i for i in response)
        assert all("topic" in i for i in response)
        assert all("content" in i for i in response)
        for i in response:
            assert isinstance(i["thread_id"], str)
            assert isinstance(i["user_id"], str)
            assert isinstance(i["date"], str)
            assert isinstance(i["topic"], str)
            assert isinstance(i["content"], list)
            inner_content = i["content"]
            # The content is a list of Stream Variants. Each must have a variant and a content
            assert all(isinstance(j, dict) for j in inner_content)
            assert all("variant" in j for j in inner_content)
            assert all("content" in j for j in inner_content)
        


def test_use_rw_dir():
    ''' Does the LLM understand how it can use the rw directory? ''' # Since Version 1.9.0
    # The rw directory is a directory that the LLM can use to store and load files for the user.
    # This is a test to see if the LLM can use it correctly.
    # It should also infer that if the user wants to save a file, it should use the rw directory.
    #TODO: remove the hint for user_id and thread_id and make sure it still works
    response = generate_full_response("This is a test. Please generate a plot of a sine wave from -2π to 2π and save it as a PNG file. Remember to save it in the proper location with user_id and thread_id.", chatbot="gpt-4.1-mini")
    # print(response)
    # Afer this, it should have generated a file in the rw directory.
    # Specifically, at "rw_dir/testing/{thread_id}/????.png"
    # So we check whether that directory exists and contains a file.
    thread_id = response.thread_id
    rw_dir = f"rw_dir/testing/{thread_id}"
    print(f"Debug: rw_dir: {rw_dir}") # Debugging
    assert os.path.exists(rw_dir), f"RW directory {rw_dir} does not exist!"

    # Make sure there is at least one file in the directory
    files = os.listdir(rw_dir)
    print(f"Debug: Files in rw_dir: {files}") # Debugging
    assert len(files) > 0, f"RW directory {rw_dir} is empty!"
    


def test_user_vision():
    ''' Can the LLM see the output that it generated? ''' # Since Version 1.10.0

    # The LLM should be able to see the image that the code it wrote generated.
    response = generate_full_response("You should have access to vision capabilities. To test them, please generate two random numbers, x and y, between -1 and 1, without printing them, and plot a big red X at the position (x, y) in a 100x100 pixel image. Then please tell me where the X is located in the image, whether it's up, down, left, right or in the center. Do not print the coordinates, save the image somehwere or write any code except for the plotting of the X! Look at the generated image instead.", chatbot="gpt-4.1-mini")

    # print(response) # Debug

    # The response should contain an image and the assistant should not be confused about the location of the X.
    assert response.image_variants, "No image variants found in response!"
    negatives = ["i don't know", "i can't see", "i can't tell", "i'm not sure", "i don't understand", "unfortunately", "i don't", "i cannot", "i can't"]
    assert not any(neg in i.lower() for i in response.assistant_variants for neg in negatives), "Assistant was confused about the location of the X! It either refused or couldn't see it."

    # Also make sure that the assistant didn't print out the coordinates. 
    # For that, test the code output for numbers, that is 0.[0-9]+
    assert not any("0." in i for i in response.codeoutput_variants), "Assistant printed out the coordinates of the X! It should only describe the location in words, not numbers."

    # Lastly make sure it actually generated an answer
    valid_answers = ["up", "down", "left", "right", "center"]
    # assert any(i.lower() in valid_answers for i in response.assistant_variants), "Assistant did not return a valid answer about the location of the X! It should have returned one of: " + ", ".join(valid_answers) + ". Instead, it returned: " + ", ".join(response.assistant_variants)
    assert any([v in ("".join(response.assistant_variants)).lower() for v in valid_answers]), "Assistant did not return a valid answer about the location of the X! It should have returned one of: " + ", ".join(valid_answers) + ". Instead, it returned: " + ", ".join(response.assistant_variants)


def test_non_alphanumeric_user_id():
    ''' Can the backend handle non-alphanumeric user IDs? ''' # Since Version 1.10.1
    # The backend should be able to handle non-alphanumeric user IDs, such as emails. 
    # This is a regression test for a bug that was introduced in Version 1.10.0, where the backend would fail to create the rw directory if the user ID contained non-alphanumeric characters
    # and then failed fully.

    try:
        # First, we need to set the user ID to a non-alphanumeric value.
        global global_user_id
        global_user_id = "example@web.de" # This is a valid email address, but contains non-alphanumeric characters.
        # Now we can run the test
        response = generate_full_response("This is simple test. Please just return 'OK' and exit.", chatbot="gpt-4.1-mini")
        # The response should contain "OK"
        assert any("OK" in i for i in response.assistant_variants), "Assistant did not return 'OK'! Instead, it returned: " + ", ".join(response.assistant_variants)
    finally:
        # Reset the user ID to the default value, so that the other tests can run without issues.
        global_user_id = "testing"
        

def test_get_user_threads_with_n():
    ''' Can the Frontend request a specific number of threads of a user? ''' # Since Version TODO
    # This test is again dependant on MongoDB being turned on.
    if should_test_mongo:
        # We'll ask for 12 threads and assume their format is correct as the other test already tested that. 
        response = get_user_threads(num_threads = 12)
        assert len(response) == 12, "Expected 12 threads, but got: " + str(len(response))


# --------------------------------
# -- Mock Authentication Server --
# --------------------------------

# In the update 1.10.0, the frevaGPT backend was updated to use a proper authentication server.
# While in production, this is populated by nginx, in testing, we will use a mock authentication server.

from flask import Flask, jsonify

app = Flask(__name__)
@app.route("/", methods=["GET"])
def auth():
    # This would usually be a proper authentication check, but for testing, we just return a dummy response.
    # The backend needs to retrieve the MongoDB URI from here, so we return the credentials to the local MongoDB instance.
    username = os.getenv("LOCAL_MONGODB_USER", "testing")
    password = os.getenv("LOCAL_MONGODB_PASSWORD", "testing")
    
    return jsonify({
        "mongodb.url": f"mongodb://{username}:{password}@localhost:27017",
    }), 200

@app.route("/api/freva-nextgen/auth/v2/systemuser", methods=["GET"])
def systemuser():
    # This would usually return the user information, but for testing, we just return a dummy response.
    # The backend needs to retrieve the user ID from here, so we return a dummy user ID.
    return jsonify({
        "user_id": global_user_id,
        "pw_name": "testing",
    }), 200

# Run the mock authentication server
def run_auth_server():

    # Wait for the mongoDB server to be up. 
    import time
    attempts = 0
    while attempts < 5:
        try:
            response = requests.get("http://localhost:27017/")
            if response.status_code == 200:
                print("MongoDB is up and running.")
                break
        except requests.ConnectionError:
            print("Waiting for MongoDB to start...")
            time.sleep(1)
            attempts += 1
    else:
        raise RuntimeError("MongoDB did not start in time.")

    app.run(port=5001, debug=True, use_reloader=False)  # Use a different port than the main server


@pytest.fixture(scope="session", autouse=True)
def setup_auth_server():
    import multiprocessing
    auth_thread = multiprocessing.Process(target=run_auth_server)
    auth_thread.start()

    # Wait for the server to start
    import time
    time.sleep(1)
    attempts = 0
    while attempts < 5:
        try:
            response = requests.get("http://localhost:5001/")
            if response.status_code == 200:
                print("Mock authentication server is up and running.")
                break
        except requests.ConnectionError:
            print("Waiting for mock authentication server to start...")
            time.sleep(1)
            attempts += 1
    else:
        raise RuntimeError("Mock authentication server did not start in time.")

    # Yield to allow tests to run
    yield

    # Teardown
    auth_thread.terminate()
    auth_thread.join(timeout=1)  # Wait for the thread to finish, if it doesn't, just ignore it.
