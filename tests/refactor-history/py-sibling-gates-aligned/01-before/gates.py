def require_save_activity(activity):
    assert activity.controller_lambda_observations > 0
    assert 0 < activity.controller_lambda_mean <= 1
    assert activity.input_effect_mean > 0


def verify_release_activity(activity):
    assert activity.controller_lambda_observations > 0
    assert 0 < activity.controller_lambda_mean <= 1
    assert activity.input_effect_mean > 0
