#!/usr/bin/env bash

set -euo pipefail

if [[ "$#" -ne 3 ]]; then
    echo "Usage: $0 <hugepages> <hugepages-group-name> <hugepages-group-id>"
    exit 1
fi

NR_HUGEPAGES="$1"

if [[ ! "$NR_HUGEPAGES" =~ ^[0-9]+$ ]]; then
    echo "Invalid number of hugepages: $NR_HUGEPAGES."
    exit 1
fi

HUGEPAGES_GROUP_NAME="$2"
HUGEPAGES_GROUP_ID="$3"

if [[ ! "$HUGEPAGES_GROUP_ID" =~ ^[0-9]+$ ]]; then
    echo "Invalid group ID: $HUGEPAGES_GROUP_ID."
    exit 1
fi

[ "$(id -u)" -ne 0 ] && exec sudo "$0" "$@"

CUR_HUGEPAGES=$(cat /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages)

if [[ "$CUR_HUGEPAGES" -lt "$NR_HUGEPAGES" ]]; then
    echo "Setting number of hugepages to $NR_HUGEPAGES."
    echo "$NR_HUGEPAGES" > /sys/kernel/mm/hugepages/hugepages-2048kB/nr_hugepages
fi

if [[ -z "$(getent group "$HUGEPAGES_GROUP_NAME" || true)" ]]; then
    echo "Creating group $HUGEPAGES_GROUP_NAME with ID $HUGEPAGES_GROUP_ID"
    groupadd -g "$HUGEPAGES_GROUP_ID" "$HUGEPAGES_GROUP_NAME"

    echo "Adding user $USER to group $HUGEPAGES_GROUP_NAME"
    usermod -a -G "$HUGEPAGES_GROUP_NAME" "$USER"

    echo "Run the command 'newgrp spdk' to apply the group change to the current login session."
fi

if [[ -z "$(findmnt -n -o SOURCE -M /mnt/hugepages || true)" ]]; then
    echo "Mounting hugetlbfs at /mnt/hugepages."
    mount -t hugetlbfs -o gid="$HUGEPAGES_GROUP_NAME",mode=777,pagesize=2M nodev /mnt/hugepages
fi
