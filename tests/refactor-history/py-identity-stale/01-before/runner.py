def resume_run(state):
    return spawn_process(state)


def publish_status(state):
    status.publish(current_pid(state))


def current_pid(state):
    return spawn_process(state)
