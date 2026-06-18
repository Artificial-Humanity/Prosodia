""" from https://github.com/jaywalnut310/glow-tts """

import numpy as np
import torch


def sequence_mask(length, max_length=None):
    if max_length is None:
        max_length = length.max()
    x = torch.arange(max_length, dtype=length.dtype, device=length.device)
    return x.unsqueeze(0) < length.unsqueeze(1)


def fix_len_compatibility(length, num_downsamplings_in_unet=2):
    factor = torch.scalar_tensor(2).pow(num_downsamplings_in_unet)
    length = (length / factor).ceil() * factor
    if not torch.onnx.is_in_onnx_export():
        return length.int().item()
    else:
        return length


def convert_pad_shape(pad_shape):
    inverted_shape = pad_shape[::-1]
    pad_shape = [item for sublist in inverted_shape for item in sublist]
    return pad_shape


def generate_path(duration, mask):
    device = duration.device

    b, t_x, t_y = mask.shape
    cum_duration = torch.cumsum(duration, 1)

    if torch.onnx.is_in_onnx_export():
        cum_duration_1d = cum_duration.squeeze(0)
        x = torch.arange(t_y, dtype=cum_duration.dtype, device=device)
        ones_x = torch.ones(t_x, dtype=cum_duration.dtype, device=device)
        ones_y = torch.ones(t_y, dtype=cum_duration.dtype, device=device)
        x_grid = torch.outer(ones_x, x)
        dur_grid = torch.outer(cum_duration_1d, ones_y)
        path_2d = (x_grid < dur_grid).to(mask.dtype)
        path_2d = path_2d - torch.nn.functional.pad(path_2d, (0, 0, 1, 0))[:-1]
        path = path_2d.unsqueeze(0)
    else:
        x = torch.arange(t_y, dtype=cum_duration.dtype, device=device)
        path = (x.view(1, 1, -1) < cum_duration.unsqueeze(-1)).to(mask.dtype)
        path = path - torch.nn.functional.pad(path, (0, 0, 1, 0, 0, 0))[:, :-1]

    path = path * mask
    return path


def duration_loss(logw, logw_, lengths):
    loss = torch.sum((logw - logw_) ** 2) / torch.sum(lengths)
    return loss


def normalize(data, mu, std):
    if not isinstance(mu, (float, int)):
        if isinstance(mu, list):
            mu = torch.tensor(mu, dtype=data.dtype, device=data.device)
        elif isinstance(mu, torch.Tensor):
            mu = mu.to(data.device)
        elif isinstance(mu, np.ndarray):
            mu = torch.from_numpy(mu).to(data.device)
        mu = mu.unsqueeze(-1)

    if not isinstance(std, (float, int)):
        if isinstance(std, list):
            std = torch.tensor(std, dtype=data.dtype, device=data.device)
        elif isinstance(std, torch.Tensor):
            std = std.to(data.device)
        elif isinstance(std, np.ndarray):
            std = torch.from_numpy(std).to(data.device)
        std = std.unsqueeze(-1)

    return (data - mu) / std


def denormalize(data, mu, std):
    if not isinstance(mu, float):
        if isinstance(mu, list):
            mu = torch.tensor(mu, dtype=data.dtype, device=data.device)
        elif isinstance(mu, torch.Tensor):
            mu = mu.to(data.device)
        elif isinstance(mu, np.ndarray):
            mu = torch.from_numpy(mu).to(data.device)
        mu = mu.unsqueeze(-1)

    if not isinstance(std, float):
        if isinstance(std, list):
            std = torch.tensor(std, dtype=data.dtype, device=data.device)
        elif isinstance(std, torch.Tensor):
            std = std.to(data.device)
        elif isinstance(std, np.ndarray):
            std = torch.from_numpy(std).to(data.device)
        std = std.unsqueeze(-1)

    return data * std + mu
