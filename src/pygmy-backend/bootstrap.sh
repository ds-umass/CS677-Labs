#!/bin/bash
echo "Running Pygmy Backend"

if [[ $1 == "catalog" ]] 
    then
        echo "Run catalog server"
        catalog-server
    else
        echo "Run order server"
        order-server
fi