// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.presentation;

public interface INodePresentation extends com.change_vision.jude.api.inf.presentation.IPresentation {
    public abstract java.awt.geom.Point2D getLocation();
    public abstract double getWidth();
    public abstract double getHeight();
    public abstract java.awt.geom.Rectangle2D getRectangle();
    public abstract void setLocation(java.awt.geom.Point2D p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setWidth(double p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract void setHeight(double p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
    public abstract com.change_vision.jude.api.inf.model.IElement getModel();
    public abstract java.lang.String getType();
    public abstract java.util.HashMap getProperties();
    public abstract void setProperty(java.lang.String p0, java.lang.String p1) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
